#!/usr/bin/env python3
"""审计「活文档」里的硬编码测试/检查计数（ADR-021 决策 4）。

## 为什么需要这个门禁

文档里写 `cargo test -p X --lib` **116 passed** 的那一刻是对的，之后每次加测试
它就变成错的。更糟的是：它**不会自己变红**——没有命令跑它、没有断言盯着它，
读者只能相信那个数字。ADR-021 之前，仓库里同一件事被记成两个互相矛盾的数
（`quality_record` 单测「6 passed」与「2026-11-14 起为 7 passed」并列），正是
这种漂移的实例。

计数本身不是问题，「把会漂移的数当成当前事实」才是。所以规则是：

    活文档只写**命令**与**结论**；要写数字，必须同时写明「以实跑输出为准」，
    或者明确标注这是**归档运行的判定**（并给出归档路径）。

数字仍然可以出现在**证据文档**里——那些记录的是某次真实运行的输出，改写它们
等于伪造历史。本脚本因此只扫「描述当前状态」的文档。

## 扫描范围

- `README.md`、`CONTRIBUTING.md`、`AGENTS.md`
- `book/src/**/*.md`
- `docs/plans/**/*.md`
- `docs/runbooks/**/*.md`
- `docs/known-issue-*.md`

**不扫**：`docs/benchmarks/**`（含 `runs/` 原始输出）、`docs/adr/**`、
`docs/project-status-*`、`docs/project-retrospective-*`、以及**带日期后缀的
历史计划**（`docs/*-plan-20*.md`）等历史档案。ADR-021 明确不追溯改写这些文档。

带日期的计划列入排除范围是**显式**的，不是为了躲门禁：这类文档记录的是写作
时点的实跑输出，和 `project-status-*` 同性质。此前它们既不在 `DOC_GLOBS`
里、也不在 `EXCLUDE_PREFIXES` 里——靠「glob 恰好没覆盖」而不是靠声明，属于
静默漏扫：任何人改 glob 就会意外把它们纳入扫描，然后被迫改写历史计数。

## 判定

一行里出现下列任一形态即视为「硬编码计数」，除非同一行带豁免标记：

- `116 passed` / `3 passed / 0 failed`
- `PASS=12`（`l3-mcp-check.sh` 这类脚本的汇总行）
- `41/41 PASS` / `2×3/3` 之外常见的 `N/M PASS`
- `11 个单测` / `8 单测`

豁免标记（同一行内，任选其一）：

- `以实跑输出为准`
- `归档判定`（历史运行的判定，需同时有归档路径）
- `计数口径`

为什么要求**同一行**而不是文件头：文件头声明很容易变成「写一次，后面所有新增
的行都自动豁免」——那正是静默放行。逐行要求豁免，成本是一句话，收益是每条
带数字的断言都被迫说清它的口径。

用法：
    python3 scripts/audit_doc_counts.py               # 校验（CI / make audit）
    python3 scripts/audit_doc_counts.py --self-check  # 判据自检
    python3 scripts/audit_doc_counts.py --json
退出码：0 通过；1 违例；2 自检失败。
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import sys
from pathlib import PurePosixPath

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

DOC_GLOBS = (
    "README.md",
    "CONTRIBUTING.md",
    "AGENTS.md",
    "book/src/**/*.md",
    "docs/plans/**/*.md",
    "docs/runbooks/**/*.md",
    "docs/known-issue-*.md",
)

# 明确排除的历史档案（ADR-021「不追溯改写」）。
# 目录前缀按 startswith 匹配；含 `*` 的模式按路径感知 glob 匹配
# （`*` 不跨目录、必须从仓库根匹配整条路径）。
EXCLUDE_PREFIXES = (
    "docs/benchmarks/",
    "docs/adr/",
    "docs/project-status-",
    "docs/project-retrospective-",
    # 带日期后缀的历史计划：记录写作时点的实跑输出，与 project-status-* 同性质。
    # 注意只覆盖 docs/ 顶层（`docs/plans/` 下的活跃计划仍需逐行标注口径）。
    "docs/*-plan-20*.md",
)

COUNT_PATTERNS = (
    # `116 passed`、`3 passed / 0 failed`
    re.compile(r"\b\d+\s+passed\b", re.IGNORECASE),
    # `PASS=12`、`FAIL=0` 之外的汇总；只认 PASS= 前缀的形态
    re.compile(r"\bPASS\s*=\s*\d+\b"),
    # `41/41 PASS`、`2/2 PASS`
    re.compile(r"\b\d+\s*/\s*\d+\s*PASS\b"),
    # `11 个单测`、`8 单测`、`21 个单元测试`
    re.compile(r"\b\d+\s*个?\s*(?:单测|单元测试)\b"),
    # `N PASS`（不带斜杠，如 `7 PASS`），避免吞掉正文里的 "PASS 证据"
    re.compile(r"\b\d+\s+PASS\b"),
)

WAIVERS = (
    "以实跑输出为准",
    "归档判定",
    "计数口径",
)


def is_excluded(rel_path: str) -> bool:
    path = PurePosixPath(rel_path)
    for pattern in EXCLUDE_PREFIXES:
        if any(ch in pattern for ch in "*?["):
            # 路径感知 glob：`PurePosixPath.match` 允许模式匹配路径**后缀**
            # （`a/docs/x.md` 也会匹配 `docs/*.md`），所以额外要求分量数
            # 一致，确保模式从仓库根匹配整条路径。
            if path.match(pattern) and len(path.parts) == len(
                PurePosixPath(pattern).parts
            ):
                return True
        elif rel_path.startswith(pattern):
            return True
    return False


def classify_line(line: str) -> str | None:
    """返回命中的计数形态（用于报告），未命中返回 None。

    豁免标记在同一行时返回 None —— 豁免是逐行的（见模块 docstring）。
    """
    if any(w in line for w in WAIVERS):
        return None
    for pattern in COUNT_PATTERNS:
        m = pattern.search(line)
        if m is not None:
            return m.group(0)
    return None


def audit_text(text: str, rel_path: str = "<memory>") -> list[dict]:
    """审计一段文档文本，返回违例列表。"""
    problems = []
    for lineno, line in enumerate(text.splitlines(), 1):
        hit = classify_line(line)
        if hit is None:
            continue
        problems.append(
            {
                "path": rel_path,
                "line": lineno,
                "hit": hit,
                "text": line.strip()[:200],
            }
        )
    return problems


def collect_paths() -> list[str]:
    paths = []
    for pattern in DOC_GLOBS:
        for p in glob.glob(os.path.join(ROOT, pattern), recursive=True):
            rel = os.path.relpath(p, ROOT).replace(os.sep, "/")
            if is_excluded(rel):
                continue
            paths.append(rel)
    return sorted(set(paths))


def _self_check() -> int:
    """判据自检：正反例各跑一遍，确保规则不会静默空转。"""
    failures = []

    def check(desc: str, text: str, expect_hits: int) -> None:
        got = audit_text(text)
        if len(got) != expect_hits:
            failures.append(
                "%s: 期望 %d 处违例，实际 %d 处（%r）"
                % (desc, expect_hits, len(got), [g["hit"] for g in got])
            )

    # ── 正例：这些必须被拦下 ──
    check("裸 passed 计数", "- `cargo test -p X --lib` 116 passed", 1)
    check("passed/failed 混合", "→ 11 passed / 0 failed", 1)
    check("脚本汇总 PASS=", "原始输出：`runs/x.out`（`PASS=12 FAIL=0`）", 1)
    check("斜杠 PASS 计数", "完整 `l3-mcp-check.sh` **65/65 PASS**", 1)
    check("个单测计数", "新增 `post_edit.rs`；11 个单测；", 1)
    check("裸单测计数", "`sisyphus_types::usage_scope`（8 单测）", 1)
    check("裸 N PASS", "实跑：s3 单独复跑 4 PASS", 1)

    # ── 反例：这些不能误报 ──
    check("命令 + 口径声明", "`cargo test -p X --lib` 通过（计数以实跑输出为准）", 0)
    check("归档判定声明", "三场景 × 3 轮 = 9/9 PASS（归档判定，见 `runs/`）", 0)
    check("计数口径声明", "> 计数口径：只记录命令与结论", 0)
    check("无计数的结论", "`cargo test -p X --lib` 通过", 0)
    check("正文里的 PASS 词", "L3 验收出现 PASS 证据（≥3 个场景）", 0)
    check("版本号里的数字", "v0.4.0 / `51503830c53b` 归档", 0)
    check("行号与端口", "`GET /health` → `{\"status\":\"SERVING\"}` 127.0.0.1:8080", 0)

    # ── 范围规则：历史档案不得被扫（否则会逼着改写历史） ──
    for rel in (
        "docs/benchmarks/acceptance/x/runs/a.md",
        "docs/adr/021-x.md",
        "docs/dev-plan-2026-10-02.md",
        "docs/optimization-plan-2026-11-05.md",
    ):
        if not is_excluded(rel):
            failures.append("范围规则退化：%s 应被排除" % rel)
    if is_excluded("docs/plans/single-node-quality-metrics.md"):
        failures.append("范围规则退化：docs/plans 下的活文档不应被排除")
    # 带日期计划的 glob 不得跨进子目录（docs/plans/ 是活文档区）
    if is_excluded("docs/plans/automation-development-plan-2026-11.md"):
        failures.append("范围规则退化：glob 不得跨目录误排除 docs/plans/")

    # ── 规则表自身不得空转 ──
    if not COUNT_PATTERNS:
        failures.append("COUNT_PATTERNS 为空：门禁会静默通过")
    if not WAIVERS:
        failures.append("WAIVERS 为空：所有带数字的行都会误报")
    if not collect_paths():
        failures.append("扫描范围为空：DOC_GLOBS 写错了，门禁在静默空转")

    if failures:
        for f in failures:
            print("❌ " + f, file=sys.stderr)
        return 2
    print("✅ 判据自检通过（正例 / 反例 / 范围规则全部符合）")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="审计活文档里的硬编码计数（ADR-021）")
    parser.add_argument("--self-check", action="store_true", help="只跑判据自检")
    parser.add_argument("--json", action="store_true", help="输出 JSON")
    args = parser.parse_args()

    if args.self_check:
        return _self_check()

    paths = collect_paths()
    if not paths:
        print("未找到任何待审计文档（DOC_GLOBS 写错？）", file=sys.stderr)
        return 1

    problems: list[dict] = []
    for rel in paths:
        try:
            with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
                text = f.read()
        except OSError as e:
            print("读取失败 %s: %s" % (rel, e), file=sys.stderr)
            return 1
        problems.extend(audit_text(text, rel))

    if args.json:
        print(json.dumps({"scanned": len(paths), "problems": problems}, ensure_ascii=False, indent=2))
    else:
        for p in problems:
            print("%s:%d: 硬编码计数 %r —— %s" % (p["path"], p["line"], p["hit"], p["text"]))
        if problems:
            print(
                "\n❌ %d 处硬编码计数（扫描 %d 个文档）。改为「命令 + 结论」，或写明"
                "「计数以实跑输出为准」/「归档判定」。" % (len(problems), len(paths)),
                file=sys.stderr,
            )
        else:
            print("✅ 活文档无硬编码计数（扫描 %d 个文档）" % len(paths))

    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
