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
import os
import pathlib
import re
import shutil
import sys
import tempfile

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


def _workspace_crates(root: pathlib.Path | None = None) -> list[pathlib.Path]:
    """列出所有「参与 Cargo 构建」的 crate 目录，判据只有一个：存在 Cargo.toml。

    不再按目录名硬编码，也不再按目录名排除——那正是漏洞的来源：曾经
    ide/ 下 4 个 crate 要靠一条单独的「非 crates/ 下的成员」分支才能被看见。

    这里刻意**不**用 Cargo.toml 的 workspace members 列表：ide/ 下的
    sdk/api/tauri 是独立构建的历史归档方向（不在 workspace 里），但它们
    确实会被各自 `cargo build` 编译，孤儿检查对它们有意义。而 ide/zed 没有
    Cargo.toml（Zed 扩展由 Zed 自身构建），不参与 Cargo，自然也不该进来。

    必须排除 root 自身：仓库根也有 Cargo.toml（虚拟 workspace 清单），
    若把它算作 crate 目录，ROOT 下每个 .rs 都会「属于某个 crate」，
    于是 tools/dummy_tool.rs 这类散落文件永远不会被检出。

    root 参数只为自检存在：_self_check() 用临时目录跑同一条判据。

    **2026-10-08 修正遍历方式**：原实现是

        base.glob("**/Cargo.toml")

    再用 `if "target" not in mf.parts` 过滤。实测这条**需要 924.7 秒**，
    而同一目录下 _source_dirs() 只要 0.0 秒——因为过滤写在了**结果**上，
    遍历照样发生：target/ 里有 15 万个编译产物、目录又深，glob 必须先走完整
    棵树才能把每个命中的路径交给那行 if 判断。孤儿门禁因此从「几秒」变成
    「十几分钟」，实测连续跑 31 分钟仍未结束。

    改为 os.walk + **遍历时**剪枝：target/、node_modules/、.git/ 一进入就不
    下探。剪枝判据取自 .gitignore（构建产物与依赖树都不该含 Cargo.toml），
    不是为了让时间好看而挑出来的。
    """
    base = root if root is not None else ROOT
    pruned = {"target", "node_modules", ".git"}
    found: list[pathlib.Path] = []
    for dirpath, dirnames, filenames in os.walk(base):
        # 就地裁剪子目录列表，os.walk 才会停止进入——这是剪枝生效的唯一方式
        dirnames[:] = [d for d in dirnames if d not in pruned]
        if "Cargo.toml" in filenames:
            crate_dir = pathlib.Path(dirpath)
            # 仓库根也有 Cargo.toml（虚拟 workspace 清单），若算作 crate，
            # ROOT 下每个 .rs 都「属于某个 crate」，散落的孤儿永远检不出来。
            if crate_dir != base:
                found.append(crate_dir)
    return sorted(found)


def _candidate_files(crates: list[pathlib.Path], source_dirs: list[pathlib.Path]) -> set[pathlib.Path]:
    """收集需要 mod 声明才能参与编译的源文件。

    三处来源：
      1. crate 顶层的散落 .rs（bin-only 布局，如 evals/）；
      2. crate 的 src/ 下的全部 .rs；
      3. **不属于任何 crate 的**散落 .rs（如 tools/dummy_tool.rs）。

    刻意不收 tests/ benches/ examples/——那些 .rs 每个都是 Cargo 自动编译
    的独立目标，本来就不需要 mod 声明，纳入会产生 191 个误报。
    """
    out: set[pathlib.Path] = set()
    for member in crates:
        out |= {p.resolve() for p in member.glob("*.rs")}
        src_dir = member / "src"
        if src_dir.is_dir():
            out |= {p.resolve() for p in src_dir.rglob("*.rs")}
    # crate 目录之外的散落 .rs。ide/zed/src/lib.rs 虽是 .rs，但 ide/zed 无
    # Cargo.toml（Zed 扩展由 Zed 自身构建），故不纳入——它不是「忘记 mod
    # 声明」，而是根本不走 Cargo。
    for base in source_dirs:
        for p in base.glob("*.rs"):
            if not any(cr in p.parents for cr in crates):
                out |= {p.resolve()}
    return out


def _source_dirs(root: pathlib.Path | None = None) -> list[pathlib.Path]:
    """自动发现所有含 Rust 源文件的顶层目录。

    与 audit_test_global_state.py 的 _scan_dirs() 保持同一判据：任何顶层目录
    只要直接含 .rs 文件，或其某个直接子目录下含 .rs，就视为源码目录。

    曾经的教训：硬编码 ("crates", "webui", "evals") 漏掉了 ide/（4 个 crate、
    9 个 .rs）与 tools/，于是 tools/dummy_tool.rs 这个真实孤儿长期不被发现。
    下次再加顶层目录又会重犯，所以改为发现而非列举。

    target/ 不会被误收：它自身无 .rs，且其直接子目录（debug/release/criterion…）
    下也无 .rs——编译产物只在 target/*/build/... 的深层里，不在直接子目录。

    root 参数只为自检存在，见 _workspace_crates()。
    """
    base = root if root is not None else ROOT
    dirs: list[pathlib.Path] = []
    for d in sorted(base.iterdir()):
        if not d.is_dir() or d.name.startswith("."):
            continue
        n = len(list(d.glob("*.rs"))) + len(list(d.glob("*/src/**/*.rs")))
        if n:
            dirs.append(d)
    return dirs


def crate_roots() -> list[pathlib.Path]:
    """从每个 crate 里找出全部编译入口。"""
    roots: list[pathlib.Path] = []
    for member in _workspace_crates():
        src = member / "src"
        # 常规布局：src/lib.rs、src/main.rs、src/bin/*.rs
        if src.is_dir():
            for name in CRATE_ROOTS:
                cand = src / name
                if cand.is_file():
                    roots.append(cand)
            bin_dir = src / "bin"
            if bin_dir.is_dir():
                roots.extend(sorted(bin_dir.glob("*.rs")))
        # bin-only 布局：evals/ 没有 src/，靠 Cargo.toml 的
        # [[bin]] path = "runner.rs" 把 .rs 直接放在 crate 顶层。
        # 漏掉这类会让 evals/ 的 5 个 .rs 全被误报成孤儿。
        for cand in sorted(member.glob("*.rs")):
            roots.append(cand)
    return roots


def _self_check() -> list[str]:
    """用临时目录跑**生产函数本身**，返回失败说明（空列表 = 全部通过）。

    为什么需要：门禁的判据会静默失效。本轮改覆盖率时连着踩了两个坑，
    都不是判据算错，而是**扫描范围**改错：

      1. 从三个硬编码目录扩到 ide/ 与 tools/ 时，静默漏掉了 bin-only 布局的
         evals/（无 src/，靠 Cargo.toml 的 [[bin]] path 挂 .rs），一度把 5 个
         正常文件报成孤儿；
      2. 「顺手」把 tests/ 也纳入候选，又制造 191 个误报——因为 tests/ 下
         每个 .rs 本就是 Cargo 自动编译的独立目标，不需要 mod 声明。

    两次都是改覆盖率引入的新缺陷，而当时的门禁一条都发现不了：它们全在
    crates/ 之外。所以钉住的不是某条规则，而是三类边界本身。

    关键：这里直接调用 _workspace_crates() 与 _candidate_files()，不是它们的
    副本。最初写成「另抄一份可测版本」时，自检绿灯而生产代码里的缺陷照样
    存在——副本与正本会漂移，这恰恰是自检最危险的失败模式：它给了安全感
    却没给保护。
    """
    failures: list[str] = []
    tmp = pathlib.Path(tempfile.mkdtemp())
    try:
        # ── 1. bin-only crate：.rs 直接挂顶层，无 src/ ──
        binonly = tmp / "evals_like"
        binonly.mkdir()
        (binonly / "Cargo.toml").write_text(
            '[[bin]]\nname = "eval-runner"\npath = "runner.rs"\n', encoding="utf-8")
        (binonly / "runner.rs").write_text("fn main() {}\n", encoding="utf-8")
        crates = _workspace_crates(tmp)
        if binonly not in crates:
            failures.append(
                "bin-only 布局（无 src/、靠 [[bin]] path 挂 .rs）未被识别为 crate"
                "——evals/ 的 5 个 .rs 会被误报成孤儿")
        elif (binonly / "runner.rs").resolve() not in _candidate_files(crates, _source_dirs(tmp)):
            failures.append("bin-only crate 的顶层 .rs 未进入候选集——会被漏报成孤儿")

        # ── 2. tests/ 下每个 .rs 是独立目标，不需要 mod 声明 ──
        normal = tmp / "crates_like" / "member"
        (normal / "src").mkdir(parents=True)
        (normal / "tests").mkdir(parents=True)
        (normal / "benches").mkdir(parents=True)
        (normal / "Cargo.toml").write_text("[package]\nname='m'\n", encoding="utf-8")
        (normal / "src" / "lib.rs").write_text("//\n", encoding="utf-8")
        (normal / "tests" / "integration.rs").write_text("#[test]\nfn t() {}\n", encoding="utf-8")
        (normal / "benches" / "b.rs").write_text("fn main() {}\n", encoding="utf-8")
        cands = _candidate_files(_workspace_crates(tmp), _source_dirs(tmp))
        for stray in (normal / "tests" / "integration.rs", normal / "benches" / "b.rs"):
            if stray.resolve() in cands:
                failures.append(
                    f"{stray.parent.name}/ 下的 .rs 被当成了需要 mod 声明的源文件"
                    "——Cargo 会自动编译它们，纳入会产生 191 个误报")

        # ── 3. 无 Cargo.toml 的目录（Zed 扩展等）不参与 Cargo ──
        zed = tmp / "zed_like"
        (zed / "src").mkdir(parents=True)
        (zed / "src" / "lib.rs").write_text("fn main() {}\n", encoding="utf-8")
        if zed in _workspace_crates(tmp):
            failures.append(
                "无 Cargo.toml 的目录被当成了 crate——"
                "Zed 扩展由 Zed 自身构建，不走 Cargo，不该参与孤儿判定")
        if (zed / "src" / "lib.rs").resolve() in cands:
            failures.append("无 Cargo.toml 的目录下的 .rs 进入了候选集——会产生永久误报")

        # ── 4. root 自身的 Cargo.toml（虚拟 workspace 清单）不算 crate ──
        # 否则 ROOT 下每个 .rs 都「属于某个 crate」，tools/dummy_tool.rs 这类
        # 散落文件永远不会被检出——这正是本轮最初的写法漏掉它的原因。
        (tmp / "Cargo.toml").write_text("[workspace]\nmembers = []\n", encoding="utf-8")
        loose = tmp / "tools_like"
        loose.mkdir()
        (loose / "stray.rs").write_text("fn main() {}\n", encoding="utf-8")
        crates2 = _workspace_crates(tmp)
        if tmp in crates2:
            failures.append("root 自身的 Cargo.toml 被当成了 crate——会掩盖所有散落 .rs")
        if (loose / "stray.rs").resolve() not in _candidate_files(crates2, _source_dirs(tmp)):
            failures.append(
                "crate 目录之外的散落 .rs 未进入候选集——"
                "tools/ 这类目录下的孤儿会被漏报")
    # ── 5. target/ 等构建产物目录必须被剪枝，不进入遍历 ──
        # 这不是洁癖：曾用 glob("**/Cargo.toml") + 结果侧过滤，实测要 924.7 秒
        # （target/ 有 15 万文件），而剪枝后只要 0.02 秒且命中集合完全一致。
        # 若剪枝失效，门禁不会报错，只会把 CI 拖到超时——正是最难发现的那类退化。
        # 用「在 target/ 里真放一个 Cargo.toml」做判别：剪枝生效时它不该被看见。
        (tmp / "target" / "some-crate").mkdir(parents=True)
        (tmp / "target" / "some-crate" / "Cargo.toml").write_text(
            "[package]\nname='built-artifact'\n", encoding="utf-8")
        if (tmp / "target" / "some-crate") in _workspace_crates(tmp):
            failures.append(
                "target/ 下的目录被当成了 crate——剪枝失效会让门禁遍历全部编译产物"
                "（实测 924.7 秒 vs 0.02 秒），且构建产物里的 Cargo.toml 会污染判定")

        # 剪枝判据来自 .gitignore，但「剪掉了该剪的」与「该留的都留下了」是
        # 两件事：正常位置的 crate 必须仍在集合里，否则上面那条会假绿。
        if (normal) not in _workspace_crates(tmp):
            failures.append("剪枝过头：crates_like/member 未被识别为 crate")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    return failures


def main() -> int:
    self_failures = _self_check()
    if self_failures:
        print("❌ 判据自检失败：", file=sys.stderr)
        for f in self_failures:
            print(f"  - {f}", file=sys.stderr)
        return 2

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

    # main() 直接调用 _workspace_crates() / _candidate_files()——自检检验的
    # 必须就是生产路径真正执行的那两个函数。若这里另写一份，自检就只是在
    # 检验一份没人用的代码。
    candidates: set[pathlib.Path] = _candidate_files(_workspace_crates(), _source_dirs())

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
