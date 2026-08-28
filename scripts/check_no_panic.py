#!/usr/bin/env python3
"""扫描生产路径（测试模块之外）的 unwrap/expect/panic! 调用。

实现 ADR-007 §1 自动化: "零 panic 路径"。

设计原则:
- 简单可审计: 纯 Python + 正则, 无外部依赖
- 准确排除 test: 跟踪 #[cfg(test)] mod 的 brace 深度
- 注释豁免: // 注释 + #[allow(...)] 不算违规
- 机器可读: --json 模式 + 明确 exit code (0=通过, 1=发现 panic)
- 基准化: --baseline mode 对比上次扫描结果 (新增 panic 即 fail)

用法:
    python3 scripts/check_no_panic.py                    # 默认扫 crates/
    python3 scripts/check_no_panic.py crates/sisyphus-tools-llm/
    python3 scripts/check_no_panic.py --json | jq
    python3 scripts/check_no_panic.py --baseline .sisyphus/quality/panic-baseline.json
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path

# 匹配 .unwrap() / .expect() 调用 (避免匹配到函数名)
UNWRAP_RE = re.compile(r'(?<![\"\'])\.(unwrap|expect)\s*\(')

# 匹配 panic! 宏 (含 panic!("..."), panic_any!(...))
PANIC_RE = re.compile(r'\bpanic!|\bpanic_any!|\bunimplemented!|\btodo!|\bunreachable!')

# 触发 test 模式的注解
TEST_ATTR_RE = re.compile(r'#\[(cfg\(test\)|test|tokio::test|case)\]')

# 豁免模式: 注释行 + allow 属性
COMMENT_OR_ALLOW_RE = re.compile(r'^\s*//|allow\s*\(|#!\[allow')


def find_production_panics(path: Path) -> list[dict]:
    """扫描单个 .rs 文件, 返回生产代码中的 unwrap/expect/panic 列表.

    算法:
    - 逐行扫描
    - 跟踪 brace 深度, 进入 #[cfg(test)] mod 后跳过 (in_test=True)
    - 离开 mod 后恢复 in_test=False
    - 匹配 UNWRAP_RE / PANIC_RE
    - 排除注释行 + #[allow(...)]
    """
    in_test = False
    brace_depth = 0  # 当前 mod 块的深度
    test_brace_depth = -1  # 进入 test mod 时的深度
    hits: list[dict] = []
    text = path.read_text(errors='ignore')
    for line_no, line in enumerate(text.splitlines(), start=1):
        # 跟踪 test 状态
        if TEST_ATTR_RE.search(line):
            in_test = True
            test_brace_depth = brace_depth

        # 跟踪 brace 深度 (粗略: 数 { 和 })
        for ch in line:
            if ch == '{':
                brace_depth += 1
            elif ch == '}':
                brace_depth -= 1
                if in_test and test_brace_depth >= 0 and brace_depth <= test_brace_depth:
                    in_test = False
                    test_brace_depth = -1

        if in_test:
            continue
        if COMMENT_OR_ALLOW_RE.search(line):
            continue

        unwrap_match = UNWRAP_RE.search(line)
        panic_match = PANIC_RE.search(line)
        if unwrap_match:
            hits.append({
                "file": str(path),
                "line": line_no,
                "kind": "unwrap" if unwrap_match.group(1) == "unwrap" else "expect",
                "text": line.strip()[:120],
            })
        elif panic_match:
            hits.append({
                "file": str(path),
                "line": line_no,
                "kind": panic_match.group(0).rstrip("!").rstrip("("),
                "text": line.strip()[:120],
            })
    return hits


def scan(targets: list[str]) -> list[dict]:
    """扫描一组目标 (文件或目录), 返回所有 hit.

    排除 (按 cargo/Rust 约定):
    - tests/ 目录 (integration tests, 由 cargo test 跑)
    - benches/ 目录 (criterion benchmarks, 内部代码)
    - examples/ 目录 (用户示例, 不进二进制)
    - target/ 目录 (cargo build 产物)

    注意: 名为 sisyphus-tests 的合法 crate 不受影响 (排除项按
    `root 已是 crates 子目录 && d in EXCLUDE_DIRS` 双重判断)。
    """
    EXCLUDE_DIRS = {"tests", "benches", "examples", "target", ".git", "node_modules", "momus"}
    # 按命名约定视为 test infra 的文件 (不是 #[cfg(test)] 但仅供测试用)
    EXCLUDE_FILES_SUFFIX = ("_tests.rs", "_test.rs", "test_helpers.rs")
    # 文件名等于此值时整文件视为 test-only
    EXCLUDE_FILES_EXACT = {"tests.rs", "build.rs"}
    all_hits: list[dict] = []
    for target in targets:
        p = Path(target)
        if p.is_file():
            all_hits.extend(find_production_panics(p))
        elif p.is_dir():
            for root, dirs, files in os.walk(p):
                # 在 crate 内 (`crates/*/...`) 才应用 EXCLUDE
                parts = Path(root).parts
                in_crate_tree = "crates" in parts
                if in_crate_tree:
                    dirs[:] = [d for d in dirs if d not in EXCLUDE_DIRS]
                for f in files:
                    if not f.endswith('.rs'):
                        continue
                    if f.endswith(EXCLUDE_FILES_SUFFIX) or f in EXCLUDE_FILES_EXACT:
                        continue
                    all_hits.extend(find_production_panics(Path(root) / f))
    return all_hits


def write_baseline(hits: list[dict], path: Path) -> None:
    """把当前 hit 列表写入 baseline 文件."""
    path.parent.mkdir(parents=True, exist_ok=True)
    # 排序以保证稳定
    sorted_hits = sorted(hits, key=lambda h: (h["file"], h["line"]))
    path.write_text(json.dumps(sorted_hits, indent=2, ensure_ascii=False))


def compare_baseline(current: list[dict], baseline_path: Path) -> tuple[list[dict], list[dict]]:
    """对比 baseline: 返回 (新增, 已删除)."""
    if not baseline_path.exists():
        return current, []
    try:
        baseline = json.loads(baseline_path.read_text())
    except json.JSONDecodeError:
        return current, []
    cur_set = {(h["file"], h["line"], h["kind"]) for h in current}
    base_set = {(h["file"], h["line"], h["kind"]) for h in baseline}
    added = [h for h in current if (h["file"], h["line"], h["kind"]) not in base_set]
    removed = [h for h in baseline if (h["file"], h["line"], h["kind"]) not in cur_set]
    return added, removed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("targets", nargs="*", default=["crates"], help="文件或目录")
    parser.add_argument("--json", action="store_true", help="JSON 输出")
    parser.add_argument("--baseline", type=Path, help="baseline 路径 (对比模式)")
    parser.add_argument("--write-baseline", type=Path, help="写入 baseline 后退出 0")
    parser.add_argument("--quiet", action="store_true", help="只输出汇总 (无 hits 详情)")
    args = parser.parse_args()

    hits = scan(args.targets)

    if args.write_baseline:
        write_baseline(hits, args.write_baseline)
        print(f"baseline 写入: {args.write_baseline} ({len(hits)} hits)")
        return 0

    if args.baseline:
        added, removed = compare_baseline(hits, args.baseline)
        if args.json:
            print(json.dumps({"added": added, "removed": removed}, indent=2, ensure_ascii=False))
        else:
            if added:
                print(f"✗ 新增 panic 路径 ({len(added)}):")
                for h in added:
                    print(f"  {h['file']}:{h['line']} [{h['kind']}] {h['text']}")
            else:
                print("✓ 无新增 panic 路径")
            if removed:
                print(f"  (已删除 {len(removed)} 个旧违规 — 建议更新 baseline)")
        return 1 if added else 0

    if args.json:
        print(json.dumps({"total": len(hits), "hits": hits}, indent=2, ensure_ascii=False))
    else:
        if hits:
            print(f"✗ 失败: {len(hits)} 个 panic 路径")
            for h in hits[:20]:
                print(f"  {h['file']}:{h['line']} [{h['kind']}] {h['text']}")
            if len(hits) > 20:
                print(f"  ... +{len(hits) - 20} more")
        else:
            print("✓ 通过: 0 个 panic 路径")
    return 1 if hits else 0


if __name__ == "__main__":
    sys.exit(main())
