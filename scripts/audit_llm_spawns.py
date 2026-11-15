#!/usr/bin/env python3
"""执行级 token 作用域：spawn 边界审计门禁。

## 背景

`sisyphus_types::usage_scope` 已落地执行级 token 归因（task-local
`UsageScope` + `spawn_in_scope` / `attach_scope`），bridge 记录入口统一走
`run_with_usage_scope`。但 ADR-021 的「失败必须可见」精神要求：**未来新增的**
spawn 边界不能静默变成「无法归因」：未走 scope 原语的 spawn 要么无法收集到 token
要么只能收集到部分。

## 判定

对 LLM 相关 crate 的**非测试**源码，扫描所有 spawn 边界（`tokio::spawn`、
`tokio::task::spawn[_blocking]`、JoinSet/任务集 `.spawn`）：

- 语句内包含 `spawn_in_scope(` / `attach_scope(` → **propagated**，通过；
- 语句附近带 `// usage-scope: <kind>[:<reason>]` → 通过，kind 取 `propagated` /
  `no-llm:<reason>` / `entry:<reason>`；
- 两者都不满足 → **违例**：新 spawn 未声明 token 作用域状态，按「无法归因」
  处理，本次不允许静默放行。

## 扫描范围

LLM 相关 crate 白名单（含测试模块与 `src/bin/` 排除）：
`sisyphus-bridge`、`sisyphus-cli`、`sisyphus-core`、`sisyphus-intel`、
`sisyphus-tools-agent`、`sisyphus-tools-executor`、`sisyphus-tools-llm`、
`sisyphus-tools-llm-tools`。

不扫：`sisyphus-sandbox`（wasm 侧无 tokio 任务）、`sisyphus-mem`/`sisyphus-knowledge`/
`sisyphus-roadmap`（数据层不发起 LLM）。

## 豁免

- 语句已含 `spawn_in_scope(` / `attach_scope(` → 豁免；
- marker comment 豁免（kind 必填，reason 有意义时写，如 `no-llm:xxx`）；
- 豁免理由不得为空。

## 用法

    python3 scripts/audit_llm_spawns.py               # 校验（CI / make audit）
    python3 scripts/audit_llm_spawns.py --self-check  # 判据自检
    python3 scripts/audit_llm_spawns.py --json
退出码：0 通过；1 违例；2 自检失败。
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import audit_todo as at  # noqa: E402 复用 TODO 无关的通用解析器

ROOT = pathlib.Path(__file__).resolve().parent.parent

CRATES = (
    "sisyphus-bridge",
    "sisyphus-cli",
    "sisyphus-core",
    "sisyphus-intel",
    "sisyphus-tools-agent",
    "sisyphus-tools-executor",
    "sisyphus-tools-llm",
    "sisyphus-tools-llm-tools",
)

SPAWN_RE = re.compile(
    r"\btokio::(?:task::)?spawn(?:_blocking|_local)?\s*\(|\.spawn(?:_blocking)?\s*\("
)
MARK_RE = re.compile(r"usage-scope:\s*([A-Za-z][A-Za-z0-9_]*(?:[-_:][A-Za-z0-9_./-]+)*)")
PROPAGATE_RE = re.compile(r"spawn_in_scope\(|attach_scope\(")
SKIP_PATH_MARKERS = ("_tests.rs", "/tests/", "/tests", "/bin/", "/target/")
VALID_KINDS = ("propagated", "entry", "no-llm")

FN_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)"
)


def _in_excluded_part(rel: str) -> bool:
    return any(m in rel for m in SKIP_PATH_MARKERS)


def _strip_tests(lines: list[str]) -> list[tuple[int, str]]:
    out: list[tuple[int, str]] = []
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i]
        if re.match(r"\s*#\[cfg\(test\)\]", line):
            j = i
            depth = 0
            started = False
            while j < n:
                depth += lines[j].count("{") - lines[j].count("}")
                if "{" in lines[j]:
                    started = True
                j += 1
                if started and depth <= 0:
                    break
            i = j
            continue
        out.append((i + 1, line))
        i += 1
    return out


def _statements(kept: list[tuple[int, str]]) -> list[tuple[int, int, str, str, int]]:
    """返回 (start_lineno, end_lineno, text, enclosing_fn)。"""
    results: list[tuple[int, int, str, str]] = []
    k = 0
    n = len(kept)
    while k < n:
        lineno, line = kept[k]
        if not SPAWN_RE.search(line) or "Command::" in line:
            k += 1
            continue
        stmt_lines = [line]
        depth = 0
        started = False
        j = k
        ended = False
        while j < n and not ended:
            depth += stmt_lines[-1].count("(") - stmt_lines[-1].count(")")
            depth += stmt_lines[-1].count("{") - stmt_lines[-1].count("}")
            if "{" in stmt_lines[-1] or "(" in stmt_lines[-1]:
                started = True
            if started and depth <= 0 and stmt_lines[-1].rstrip().endswith(";"):
                ended = True
            if not ended:
                j += 1
                if j < n:
                    stmt_lines.append(kept[j][1])
        text = "\n".join(stmt_lines)
        fn = "?"
        for kk in range(j, k - 1, -1):
            m = FN_RE.match(kept[kk - 1][1]) if kk > 0 else None
            if m:
                fn = m.group(1)
                break
        results.append((lineno, j, text, fn, k))
        k = max(j, k + 1)
    return results


def audit(root: pathlib.Path = ROOT) -> list[dict]:
    problems: list[dict] = []
    for crate in CRATES:
        base = root / "crates" / crate / "src"
        if not base.exists():
            continue
        for p in sorted(base.rglob("*.rs")):
            rel = p.relative_to(root).as_posix()
            if _in_excluded_part(rel):
                continue
            lines = p.read_text(encoding="utf-8", errors="replace").splitlines()
            kept = _strip_tests(lines)
            for start, end, text, fn, k in _statements(kept):
                if PROPAGATE_RE.search(text):
                    continue
                if k > 0 and kept[k - 1][1].lstrip().startswith("//"):
                    text = kept[k - 1][1] + "\n" + text
                marks = MARK_RE.findall(text)
                if marks:
                    kind = marks[0].split(":", 1)[0]
                    if kind not in VALID_KINDS:
                        problems.append(
                            {
                                "kind": "unmarked_spawn",
                                "reason": "usage-scope marker kind 非法（kind=%s）" % kind,
                                "detail": "%s:%d fn=%s" % (rel, start, fn),
                                "text": " ".join(text.split())[:140],
                            }
                        )
                        continue
                    reason = marks[0].split(":", 1)[1] if ":" in marks[0] else ""
                    if not reason:
                        problems.append(
                            {
                                "kind": "unmarked_spawn",
                                "reason": "usage-scope marker 缺少 reason（kind=%s）" % kind,
                                "detail": "%s:%d fn=%s" % (rel, start, fn),
                                "text": " ".join(text.split())[:140],
                            }
                        )
                    continue
                problems.append(
                    {
                        "kind": "unmarked_spawn",
                        "reason": "spawn 未走 scope 原语且无 usage-scope marker（无法确定 token 是否归因）",
                        "detail": "%s:%d fn=%s" % (rel, start, fn),
                        "text": " ".join(text.split())[:140],
                    }
                )
    return problems


def _self_check() -> int:
    failures: list[str] = []

    def mk(files: dict[str, str]) -> pathlib.Path:
        tmp = pathlib.Path(tempfile.mkdtemp(prefix="audit-llm-spawns-"))
        for rel, content in files.items():
            p = tmp / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content, encoding="utf-8")
        return tmp

    ok_body = """
pub fn add(a: i32, b: i32) -> i32 { a + b }
#[cfg(test)]
mod tests {
    %s
}
"""
    # 正例 1：propagated（spawn_in_scope）→ 不违例
    root = mk(
        {
            "TODO.md": "# T\n",
            "crates/sisyphus-core/src/lib.rs": ok_body % (
                "#[test]\n    fn test_add() {\n        assert_eq!(super::add(1, 2), 3);\n    }\n"
            ),
            "crates/sisyphus-core/src/ex.rs": "use tokio::task::JoinSet;\nfn f() { let mut js = JoinSet::new(); js.spawn(sisyphus_types::usage_scope::attach_scope(async {})); }\n",
        }
    )
    # 正例 1b：spawn_in_scope 在同一语句 → 不违例
    root = mk(
        {
            "TODO.md": "# T\n",
            "crates/sisyphus-core/src/lib.rs": ok_body % (
                "#[test]\n    fn test_add() {\n        assert_eq!(super::add(1, 2), 3);\n    }\n"
            ),
            "crates/sisyphus-core/src/ex.rs": (
                "use tokio::task::JoinSet;\nfn f() { let mut js = JoinSet::new(); js.spawn(sisyphus_types::usage_scope::attach_scope(async {})); }\n"
            ),
        }
    )
    if audit(root):
        failures.append("propagated spawn 被误报：%r" % (audit(root),))

    # 正例 2：带 marker（entry:no-llm:reason）→ 不违例
    root = mk(
        {
            "TODO.md": "# T\n",
            "crates/sisyphus-core/src/lib.rs": ok_body % (
                "#[test]\n    fn test_add() {\n        assert_eq!(super::add(1, 2), 3);\n    }\n"
            ),
            "crates/sisyphus-core/src/ex.rs": (
                "use tokio::task::JoinSet;\nfn f() {\n    let mut js = JoinSet::new();\n    // usage-scope: entry:no-llm:reason\n    js.spawn(async move {});\n}\n"
            ),
        }
    )
    if audit(root):
        failures.append("带 marker 的 spawn 被误报：%r" % (audit(root),))

    # 正例 3：空 reason 的 marker（`usage-scope: entry`）→ 违例（reason 必填）
    root = mk(
        {
            "TODO.md": "# T\n",
            "crates/sisyphus-core/src/lib.rs": ok_body % (
                "#[test]\n    fn test_add() {\n        assert_eq!(super::add(1, 2), 3);\n    }\n"
            ),
            "crates/sisyphus-core/src/ex.rs": (
                "use tokio::task::JoinSet;\nfn f() { let mut js = JoinSet::new(); // usage-scope: entry\n js.spawn(async move {}); }\n"
            ),
        }
    )
    got = audit(root)
    if not any(p["kind"] == "unmarked_spawn" for p in got):
        failures.append("空 reason 的 marker 未被拒（reason 必填）:%r" % (got,))

    # 反例：无 marker/propagation → 违例
    root = mk(
        {
            "TODO.md": "# T\n",
            "crates/sisyphus-core/src/lib.rs": ok_body % (
                "#[test]\n    fn test_add() {\n        assert_eq!(super::add(1, 2), 3);\n    }\n"
            ),
            "crates/sisyphus-core/src/ex.rs": "use tokio::task::JoinSet;\nfn f() { let mut js = JoinSet::new(); js.spawn(async move {}); }\n",
        }
    )
    if not any(p["kind"] == "unmarked_spawn" for p in audit(root)):
        failures.append("未标记 spawn 未被拦下")

    # 规则表非空
    if not CRATES or not SPAWN_RE or not MARK_RE or not PROPAGATE_RE:
        failures.append("规则/正则表为空：门禁会静默通过")

    if failures:
        for f in failures:
            print("❌ " + f, file=sys.stderr)
        return 2
    print("✅ 判据自检通过（2 正例 / 2 反例 + 表非空）")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="执行级 token spawn 边界审计门禁")
    parser.add_argument("--self-check", action="store_true", help="只跑判据自检")
    parser.add_argument("--json", action="store_true", help="输出 JSON")
    args = parser.parse_args()
    if args.self_check:
        return _self_check()
    problems = audit(ROOT)
    if args.json:
        print(json.dumps({"problems": problems}, ensure_ascii=False, indent=2))
    else:
        for p in problems:
            print("%s: %s —— %s" % (p["kind"], p["reason"], p.get("detail", "")))
        if problems:
            print(
                "\n❌ %d 处未声明 token 作用域的 spawn（ADR-021：绝不静默放行）。"
                "补充 `spawn_in_scope`/`attach_scope` 或在 spawn 处加"
                " `// usage-scope: entry:<reason>` / `// usage-scope: no-llm:<reason>`。"
                % len(problems),
                file=sys.stderr,
            )
        else:
            print("✅ 全部 LLM crate 内未测试 spawn 均已声明 token 作用域状态")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
