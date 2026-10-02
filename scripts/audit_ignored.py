#!/usr/bin/env python3
"""审计 `#[ignore]` 的理由是否诚实。

背景：本项目有 15 处 `#[ignore]`，而 CI 从不使用 `--include-ignored`——
被忽略的测试等于没有覆盖。其中两处写着 `#[ignore = "requires dedicated
runner"]`，但它们失败的原因不是资源受限，而是内核未注入、失败被吞成成功。
把「测试红了」写成「环境受限」，会让真正的回归保护凭空消失且无人察觉。

本审计只接受「外部前置条件」作为忽略理由：网络、凭据、外部工具链、
需要运行中的服务、已声明的能力缺口。裸 `#[ignore]`（无理由）与上述之外的
理由一律报出。

用法：
    python3 scripts/audit_ignored.py
    python3 scripts/audit_ignored.py --json
退出码：有问题时为 1，否则为 0。
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
from collections import Counter

ROOT = pathlib.Path(__file__).resolve().parent.parent

IGNORE_RE = re.compile(
    r'#\s*\[\s*ignore\s*(?:=\s*"([^"]*)")?\s*\]'
)

# 被认可的前置条件关键词（不区分大小写）
ACCEPTED = (
    "network",            # 需要网络
    "api_key", "api key", # 需要凭据
    "env var", "env_var", # 需要环境变量
    "toolchain", "installed",  # 需要外部工具链
    "running", "requires a running",  # 需要运行中的服务
    "mock llm",           # 已声明的能力缺口（测试依赖尚未实现的夹具）
    "sandbox", "docker",  # 需要特定运行环境
)

SCAN_DIRS = ("crates", "webui", "evals")


def blank_comments(src: str) -> str:
    """把注释内容替换为空格但保留换行，使行号不变。

    必须做这一步：文档里常会引用 `#[ignore = "..."]` 作为说明，
    例如解释「此前带 #[ignore] 是因为测试失败」。不剥离就会把文档
    里的示例当成真实的忽略点。
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == '/' and i + 1 < n and src[i + 1] == '/':
            j = src.find('\n', i)
            j = n if j == -1 else j
            for k in range(i, j):
                out[k] = ' '
            i = j
        elif c == '/' and i + 1 < n and src[i + 1] == '*':
            j = src.find('*/', i + 2)
            j = n if j == -1 else j + 2
            for k in range(i, min(j, n)):
                if out[k] != '\n':
                    out[k] = ' '
            i = j
        elif c == '"':
            j = i + 1
            while j < n:
                if src[j] == '\\':
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            i = j
        else:
            i += 1
    return ''.join(out)


def next_ident(src: str, pos: int) -> str:
    """取 #[ignore] 之后最近的函数名，用于定位。"""
    m = re.search(r'fn\s+([A-Za-z_][A-Za-z0-9_]*)', src[pos:pos + 400])
    return m.group(1) if m else "<未知>"


def scan() -> list[dict]:
    findings: list[dict] = []
    for base in SCAN_DIRS:
        root = ROOT / base
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            try:
                raw = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            src = blank_comments(raw)
            for m in IGNORE_RE.finditer(src):
                reason = (m.group(1) or "").strip()
                line = src.count('\n', 0, m.start()) + 1
                ok = bool(reason) and any(k in reason.lower() for k in ACCEPTED)
                findings.append({
                    "file": str(path.relative_to(ROOT)),
                    "line": line,
                    "fn": next_ident(src, m.end()),
                    "reason": reason,
                    "accepted": ok,
                })
    return findings


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    items = scan()
    bad = [i for i in items if not i["accepted"]]

    if args.json:
        print(json.dumps({"ignored": items, "rejected": len(bad)},
                         ensure_ascii=False, indent=2))
        return 1 if bad else 0

    if not items:
        print("✅ 未发现 #[ignore] 测试")
        return 0

    reasons = Counter(i["reason"] or "<无理由>" for i in items)
    print(f"共 {len(items)} 处 #[ignore]：")
    for reason, count in reasons.most_common():
        print(f"  {count:>2} × {reason}")

    if not bad:
        print("\n✅ 全部忽略理由均指向外部前置条件")
        return 0

    print(f"\n❌ {len(bad)} 处忽略理由未指向外部前置条件：\n")
    for i in bad:
        print(f"  {i['file']}:{i['line']}  {i['fn']}")
        print(f"      理由：{i['reason'] or '<无理由>'}")
    print("\n被忽略的测试等于没有覆盖——CI 不会执行它们。")
    print("若原因是「测试当前失败」，应先修复测试或删除它，而不是加 #[ignore]。")
    print("可接受的理由需指向：网络、凭据/环境变量、外部工具链、需要运行中的服务，")
    print("或明确声明的能力缺口。确需专用 runner 时，请在理由中写明它依赖什么。")
    return 1


if __name__ == "__main__":
    sys.exit(main())
