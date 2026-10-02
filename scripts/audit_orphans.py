#!/usr/bin/env python3
"""扫描 src/ 下从未被任何 crate 引入的「孤儿源文件」。

背景：本项目曾出现 crates/sisyphus-mem/src/tests.rs —— 227 行、7 个测试用例，
但 lib.rs 从未 `mod tests;`，因此该文件从未参与编译，其中的断言从未运行过，
却让人误以为状态存取层有覆盖。这类缺陷靠人 review 很难发现，因为文件本身
看起来完全正常。

用法：
    python3 scripts/audit_orphans.py            # 扫描并报告
    python3 scripts/audit_orphans.py --json     # 机器可读输出
退出码：有孤儿文件时为 1，否则为 0（便于 CI 直接拦截）。
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# `mod foo;` / `pub mod foo;` / `pub(crate) mod foo;`（不含花括号，即文件模块）
MOD_DECL = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;", re.M)
# `#[path = "some/where.rs"]`，修饰紧随其后的 mod 声明
PATH_ATTR = re.compile(r'#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]')
# 顶层 crate 根
CRATE_ROOTS = ("lib.rs", "main.rs")


def strip_comments(src: str) -> str:
    """粗略去掉注释与字符串字面量，避免注释里的 mod 被误当成声明。

    这是启发式扫描而非编译器级解析：宁可放过，也不要误报。
    """
    out = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == '/' and i + 1 < n and src[i + 1] == '/':
            j = src.find('\n', i)
            i = n if j == -1 else j
        elif c == '/' and i + 1 < n and src[i + 1] == '*':
            j = src.find('*/', i + 2)
            i = n if j == -1 else j + 2
        elif c == '"':
            # 保留字面量本身：`#[path = "foo.rs"]` 的内容正是解析所需，
            # 删掉它会把整个属性变成 `#[path = ]`，导致模块解析失败。
            j = i + 1
            while j < n:
                if src[j] == '\\':
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            out.append(src[i:j])
            i = j
        else:
            out.append(c)
            i += 1
    return ''.join(out)


def child_dir(base_file: pathlib.Path) -> pathlib.Path:
    """Rust 2018 的子模块查找目录。

    crate 根（lib.rs / main.rs）与 `mod.rs` 的子模块与自身同目录；
    而 `src/llm.rs` 的子模块在 `src/llm/` 下——目录名取文件名（不含 .rs）。
    这是 Rust 2018 相对 2015 的关键变化，解析时必须区分。
    """
    parent = base_file.parent
    if base_file.name in ("lib.rs", "main.rs", "mod.rs"):
        return parent
    return parent / base_file.stem


def resolve_module(base_file: pathlib.Path, name: str, path_attr: str | None) -> pathlib.Path | None:
    """把一个 mod 声明解析为具体文件路径。"""
    base = child_dir(base_file)
    if path_attr:
        # #[path] 相对当前文件所在目录解析，而非相对子模块目录
        cand = (base_file.parent / path_attr).resolve()
        return cand if cand.is_file() else None
    cand = (base / f"{name}.rs").resolve()
    if cand.is_file():
        return cand
    cand = (base / name / "mod.rs").resolve()
    return cand if cand.is_file() else None


def reachable(entry: pathlib.Path) -> set[pathlib.Path]:
    """从 crate 根出发，沿 mod 声明遍历出所有被引入的文件。"""
    seen: set[pathlib.Path] = set()
    stack = [entry]
    while stack:
        current = stack.pop()
        try:
            current = current.resolve()
        except OSError:
            continue
        if current in seen or not current.is_file():
            continue
        seen.add(current)
        raw = strip_comments(current.read_text(encoding="utf-8", errors="replace"))
        # 把每个 #[path] 绑定到它后面紧跟的 mod 名
        pending: dict[str, str] = {}
        for m in re.finditer(
            r'#\s*\[\s*path\s*=\s*"([^"]+)"\s*\][^;]*?\bmod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;',
            raw,
            re.S,
        ):
            pending[m.group(2)] = m.group(1)
        for m in MOD_DECL.finditer(raw):
            name = m.group(1)
            target = resolve_module(current, name, pending.get(name))
            if target is not None:
                stack.append(target)
    return seen


def crate_roots() -> list[pathlib.Path]:
    roots: list[pathlib.Path] = []
    for member in ROOT.glob("crates/*"):
        if not (member / "Cargo.toml").is_file():
            continue
        src = member / "src"
        if not src.is_dir():
            continue
        for name in CRATE_ROOTS:
            cand = src / name
            if cand.is_file():
                roots.append(cand)
        bin_dir = src / "bin"
        if bin_dir.is_dir():
            roots.extend(sorted(bin_dir.glob("*.rs")))
    # 非 crates/ 下的成员
    for member in (ROOT / "webui", ROOT / "evals"):
        src = member / "src"
        if src.is_dir():
            for name in CRATE_ROOTS:
                cand = src / name
                if cand.is_file():
                    roots.append(cand)
    return roots


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true", help="输出 JSON")
    args = parser.parse_args()

    roots = crate_roots()
    if not roots:
        print("未找到任何 crate 根，无法执行扫描", file=sys.stderr)
        return 2

    reached: set[pathlib.Path] = set()
    for root in roots:
        reached |= reachable(root)

    candidates: set[pathlib.Path] = set()
    for base in [ROOT / "crates", ROOT / "webui", ROOT / "evals"]:
        if not base.is_dir():
            continue
        for src_dir in base.glob("*/src"):
            candidates |= {p.resolve() for p in src_dir.rglob("*.rs")}

    orphans = sorted(p for p in candidates - reached)

    if args.json:
        print(json.dumps(
            {"orphans": [str(p.relative_to(ROOT)) for p in orphans]},
            ensure_ascii=False, indent=2,
        ))
        return 1 if orphans else 0

    if not orphans:
        print(f"✅ 未发现孤儿源文件（扫描 {len(candidates)} 个文件，"
              f"{len(roots)} 个 crate 根）")
        return 0

    print(f"❌ 发现 {len(orphans)} 个孤儿源文件（从未被 mod 声明引入，不参与编译）：\n")
    for p in orphans:
        rel = p.relative_to(ROOT)
        lines = len(p.read_text(encoding="utf-8", errors="replace").splitlines())
        print(f"  {rel}（{lines} 行）")
    print("\n处理方式二选一：")
    print("  1. 若文件是有价值的测试/实现 → 在父模块加 #[cfg(test)] mod ...; "
          "或 #[path] mod ...; 接入")
    print("  2. 若是残留死文件 → 删除")
    return 1


if __name__ == "__main__":
    sys.exit(main())
