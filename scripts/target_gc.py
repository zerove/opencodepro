#!/usr/bin/env python3
"""回收 target/ 下的陈旧构建产物。

cargo 自带的 `cargo gc` 目前仍是 nightly-only（stable 1.99 无此子命令），
所以这里直接按「age + 可逆性」回收，规则保守：

  1. 只删**超过 --age 天**的产物（默认 14 天），且默认 dry-run；
  2. 永不触碰最近 --keep-days 天内被 cargo 写过的任何文件；
  3. 只处理已排除出默认 check/test 集合的 crate，避免把它们仅有的
     构建产物删掉——那会让下次全量重编，反而更慢。

用法：
    python3 scripts/target_gc.py                 # 预览（默认）
    python3 scripts/target_gc.py --age 21        # 只看 21 天前的
    python3 scripts/target_gc.py -y --age 14     # 真正删除
    python3 scripts/target_gc.py --self-check    # 自检保护边界

配套的体积门禁见 scripts/audit_target_size.py。
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# 这些 crate 被排除在默认 check/test 之外（见 Cargo.toml alias 与
# .cargo/config.toml 注释），单独构建。它们的产物没有别处可复用，
# 删掉等于让下一次构建全量重编，所以默认整棵跳过。
#
# 注意：目录名用连字符（sisyphus-bridge），而产物文件名前缀用下划线
# （libsisyphus_bridge-<hash>.rlib），两者都要匹配。
PROTECTED_CRATES = ("sisyphus-bridge", "sisyphus-sandbox", "sisyphus-sandboxd")


def crate_dirnames() -> tuple[str, ...]:
    """build/ 下的目录名形态：连字符。"""
    return PROTECTED_CRATES


def crate_file_prefixes() -> set[str]:
    """产物文件名前缀形态：lib 下划线 / 无前缀 下划线 / 连字符。

    还需覆盖**不带哈希后缀**的顶层二进制名（如 target/release/sisyphus_sandboxd）。

    注意不能用裸 startswith：``sisyphus_bridge`` 会连带命中
    ``sisyphus_bridge_api``（一个**正常**的 workspace 成员，不在排除集里），
    删掉它的产物等于让下次构建全量重编。边界必须是 ``前缀 + 分隔符``。
    """
    prefixes: set[str] = set()
    for name in PROTECTED_CRATES:
        underscored = name.replace("-", "_")
        for stem in {underscored, name, f"lib{underscored}"}:
            # 带哈希后缀的产物：lib<crate>-<hash>.rlib
            prefixes.add(f"{stem}-")
    return prefixes


FILE_PREFIXES = crate_file_prefixes()
# 顶层二进制既可能是 sisyphus_sandboxd，也可能带 lib 前缀（libsisyphus_sandboxd），
# 且不带哈希后缀，所以精确名要同时覆盖三种形态。
EXACT_NAMES = {
    variant
    for name in PROTECTED_CRATES
    for variant in (name, name.replace("-", "_"), f"lib{name.replace('-', '_')}")
}


def is_protected(path: Path, root: Path) -> bool:
    """判断产物是否属于被单独构建的 crate。"""
    try:
        rel = path.relative_to(root)
    except ValueError:
        return False
    parts = rel.parts
    if not parts:
        return False
    # target/debug/build/<crate>/... 或 target/<profile>/<crate>*
    if "build" in parts:
        idx = parts.index("build")
        if idx + 1 < len(parts) and parts[idx + 1] in PROTECTED_CRATES:
            return True
    # 文件名自身形如 libsisyphus_bridge-<hash>.rlib
    name = parts[-1]
    if any(name.startswith(p) for p in FILE_PREFIXES):
        return True
    # 顶层二进制无哈希后缀，如 target/release/sisyphus_sandboxd
    return any(name == stem for stem in EXACT_NAMES)


# 这些顶层目录整体陈旧时可直接删（可重新生成，且不含需保留的状态）。
REGENERABLE = ("debug/incremental", "debug/examples", "doc", "criterion")


def human(num_bytes: int) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if abs(num_bytes) < 1024.0:
            return f"{num_bytes:.1f} {unit}"
        num_bytes /= 1024.0
    return f"{num_bytes:.1f} TB"


def collect_files(root: Path, age_days: int, keep_days: int) -> tuple[list[Path], int, int]:
    """返回 (待删文件, 总数, 跳过数)。"""
    cutoff = time.time() - age_days * 86400
    keep_cutoff = time.time() - keep_days * 86400
    victims: list[Path] = []
    scanned = skipped = 0
    for dirpath, _dirnames, filenames in os.walk(root):
        current = Path(dirpath)
        for name in filenames:
            scanned += 1
            path = current / name
            try:
                stat = path.stat()
            except OSError:
                skipped += 1
                continue
            if stat.st_mtime > keep_cutoff:
                skipped += 1
                continue
            if stat.st_mtime >= cutoff:
                skipped += 1
                continue
            if is_protected(path, root):
                skipped += 1
                continue
            victims.append(path)
    return victims, scanned, skipped


def dir_size(path: Path) -> int:
    total = 0
    for dirpath, _dirnames, filenames in os.walk(path):
        for name in filenames:
            try:
                total += os.path.getsize(Path(dirpath) / name)
            except OSError:
                continue
    return total


def remove_empty_dirs(root: Path) -> int:
    removed = 0
    for dirpath, dirnames, filenames in os.walk(root, topdown=False):
        if not dirnames and not filenames:
            try:
                os.rmdir(dirpath)
                removed += 1
            except OSError:
                continue
    return removed


def reclaim_whole_dirs(target: Path, age_days: int, keep_days: int, dry_run: bool) -> tuple[int, int]:
    """整棵删除可重新生成的陈旧目录，返回 (字节, 目录数)。"""
    cutoff = time.time() - age_days * 86400
    keep_cutoff = time.time() - keep_days * 86400
    freed = dirs = 0
    for rel in REGENERABLE:
        path = target / rel
        if not path.is_dir():
            continue
        try:
            newest = max(
                (p.stat().st_mtime for p in path.rglob("*") if p.exists()), default=0.0
            )
        except OSError:
            continue
        if newest and newest < cutoff and newest < keep_cutoff:
            size = dir_size(path)
            if size == 0:
                continue
            freed += size
            dirs += 1
            if not dry_run:
                shutil.rmtree(path, ignore_errors=True)
    return freed, dirs


def self_check() -> int:
    """验证保护边界：阳性（应保护）与阴性（不应保护）两侧都要覆盖。

    只跑单元对照是不够的——曾因前缀过宽把 sisyphus_bridge_api 一并保护，
    也曾因命名形态不一致（连字符 vs 下划线）漏掉真正的 bridge 产物。
    两类错误方向相反，必须同时钉住。
    """
    root = Path("/repo")
    cases: tuple[tuple[str, bool, str], ...] = (
        # 阳性：被排除、单独构建的 crate，产物必须保留
        ("target/debug/deps/libsisyphus_bridge-abc.rlib", True, "bridge rlib"),
        ("target/debug/deps/libsisyphus_sandbox-abc.rlib", True, "sandbox rlib"),
        ("target/debug/build/sisyphus-bridge/aa/out", True, "bridge build 输出"),
        ("target/debug/build/sisyphus-sandbox/aa/out", True, "sandbox build 输出"),
        ("target/release/libsisyphus_sandboxd", True, "sandboxd bin"),
        ("target/release/sisyphus_sandboxd", True, "sandboxd 无后缀名"),
        # 阴性：正常 workspace 成员与第三方，绝不能被当成受保护
        ("target/debug/deps/libsisyphus_bridge_api-abc.rlib", False, "bridge-api 不误伤"),
        ("target/debug/deps/libsisyphus_core-abc.rlib", False, "核心 crate"),
        ("target/debug/deps/libsisyphus_tools_core-abc.rlib", False, "tools-core"),
        ("target/debug/deps/libtower-abc.rlib", False, "第三方依赖"),
        ("target/debug/.fingerprint/tower-abc/lib-tower", False, "fingerprint"),
        ("target/debug/deps/libsisyphus_tools_agent-abc.rlib", False, "tools-agent"),
    )
    failures = 0
    print("保护边界自检：")
    for rel, want, label in cases:
        got = is_protected(root / rel, root)
        ok = got == want
        failures += 0 if ok else 1
        mark = "✅" if ok else "❌"
        print(f"  {mark} {label:20} 期望={want!s:5} 实际={got}")

    # 交叉验证：前缀集合不能互相吞并
    for name in PROTECTED_CRATES:
        underscored = name.replace("-", "_")
        if f"{underscored}-" in FILE_PREFIXES:
            continue
        print(f"  ❌ {name} 缺少带分隔符的前缀")
        failures += 1

    if failures:
        print(f"\n❌ 自检失败：{failures} 项")
        return 1
    print(f"\n✅ 自检通过：{len(cases)} 项对照全部正确")
    return 0


def main() -> int:
    global PROTECTED_CRATES, FILE_PREFIXES, EXACT_NAMES
    parser = argparse.ArgumentParser(description="回收 target/ 下陈旧的构建产物")
    parser.add_argument("--age", type=int, default=14, help="删除早于多少天的产物（默认 14）")
    parser.add_argument(
        "--keep-days",
        type=int,
        default=2,
        help="最近多少天内 cargo 写过的文件一律不动（默认 2）",
    )
    parser.add_argument(
        "--dir",
        default="target",
        help="要清理的 target 目录（默认 target）",
    )
    parser.add_argument("-y", "--yes", action="store_true", help="真正删除（默认为预览）")
    parser.add_argument("--skip-protected", action="store_true", help="连受保护的 crate 也一起清")
    parser.add_argument("--self-check", action="store_true", help="只跑保护边界自检")
    args = parser.parse_args()

    if args.self_check:
        return self_check()

    target = (REPO_ROOT / args.dir).resolve()
    if not target.is_dir():
        print(f"❌ 目录不存在：{target}")
        return 1

    if args.skip_protected:
        PROTECTED_CRATES = ()
        FILE_PREFIXES = set()
        EXACT_NAMES = set()

    if not args.yes:
        print("🔍 预览模式（加 -y 才会真正删除）\n")

    print(f"正在扫描 {target}（{args.age} 天前的产物）…")
    victims, scanned, skipped = collect_files(target, args.age, args.keep_days)
    total = sum(p.stat().st_size for p in victims if p.exists())
    dirs_freed, dir_count = reclaim_whole_dirs(
        target, args.age, args.keep_days, dry_run=not args.yes
    )

    print()
    print(f"扫描文件      : {scanned}")
    print(f"可回收        : {len(victims)} 个，{human(total)}")
    print(f"可整体删除    : {dir_count} 个目录，{human(dirs_freed)}")
    print(f"保护/跳过     : {skipped}")
    print(f"合计可释放    : {human(total + dirs_freed)}")

    if not args.yes:
        print("\n这是预览。加 -y 执行。")
        return 0

    removed = 0
    freed_bytes = 0
    errors = 0
    missing = 0
    for path in victims:
        try:
            size = path.stat().st_size
        except OSError:
            # 已被整棵目录删除覆盖（如 criterion/）——不算错误。
            missing += 1
            continue
        try:
            path.unlink()
            removed += 1
            freed_bytes += size
        except OSError as exc:
            print(f"  ⚠️  {path.relative_to(target)}: {exc}")
            errors += 1

    if removed or missing:
        empty = remove_empty_dirs(target)
        detail = f"✅ 已删除 {removed} 个文件，释放 {human(freed_bytes)}"
        if missing:
            detail += f"（另有 {missing} 个已随整棵目录移除）"
        print(f"\n{detail}；清理 {empty} 个空目录")
    else:
        print("\n没有删除任何文件。")

    before = subprocess.run(["df", "-h", str(target)], capture_output=True, text=True)
    if before.returncode == 0:
        print("\n当前磁盘：")
        print(before.stdout.strip())
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())