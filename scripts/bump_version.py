#!/usr/bin/env python3
"""统一 bump workspace 版本号（`[package] version` + 内部 path 依赖的 version 约束）。

## 为什么需要脚本

版本号在仓库里出现 148 次：27 个 crate 的 `[package] version`，加上内部依赖
`sisyphus-* = { path = "...", version = "x.y.z" }` 的约束。手改必漏——历史上
`evals/Cargo.toml` 的 6 处 path 依赖就因为漏了 `version` 约束，让
`cargo package` 报 “all dependencies must have a version requirement
specified when packaging”，而 `cargo check` 完全看不出来（本地构建不需要它）。
漏改的另一个表现更隐蔽：版本号不一致时 `make release-check` 才会红。

## 判据

- `[package]` 段内的 `version = "..."`：**必须**等于目标版本；
- 依赖表里 `path = ` 指向 workspace 成员的条目：**必须**带 `version` 约束，
  且等于目标版本（缺 version 视为错误，不静默补——那会掩盖作者本意）；
- 第三方依赖的版本（`serde = "1.0"` 等）：一律不碰。

## 用法

    python3 scripts/bump_version.py 0.3.0 --dry-run   # 打印将要改的行
    python3 scripts/bump_version.py 0.3.0             # 实际写入
    python3 scripts/bump_version.py 0.3.0 --check     # 只校验（CI / bump 后自查）
    python3 scripts/bump_version.py --self-check      # 判别用例自检

退出码：0 成功 / 通过；1 校验失败；2 参数或自检错误。
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")
# `[package]` 段内的 version 行；只匹配行首（依赖行的 version 在 `{ ... }` 内）。
PKG_VERSION_RE = re.compile(r'^(\s*version\s*=\s*")([^"]*)(")\s*$')
# 依赖表的 key 行：`name = { ... }` 或 `[dependencies.name]` 形式这里不处理，
# 本仓库统一用内联表写法（由自检用例固定这一前提）。
DEP_LINE_RE = re.compile(r"^(\s*)([A-Za-z0-9_-]+)(\s*=\s*\{)(.*)(\}\s*)$")
INNER_VERSION_RE = re.compile(r'(version\s*=\s*")([^"]*)(")')
PATH_RE = re.compile(r'path\s*=\s*"([^"]*)"')


def workspace_members(root: Path) -> list[Path]:
    """workspace 成员的 manifest 路径（相对 root）。"""
    meta = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    return [root / m / "Cargo.toml" for m in meta["workspace"]["members"]]


def member_names(manifests: list[Path]) -> set[str]:
    names = set()
    for m in manifests:
        data = tomllib.loads(m.read_text(encoding="utf-8"))
        name = data["package"]["name"]
        names.add(name)
    return names


def rewrite(text: str, target: str, internal: set[str]) -> tuple[str, list[str], list[str]]:
    """改写一份 manifest 文本。

    返回 (新文本, 改动的行, 错误)。错误指「内部依赖缺 version 约束」——
    这种条目不能猜，必须让调用方知道。
    """
    out: list[str] = []
    changed: list[str] = []
    errors: list[str] = []
    section = ""

    for line in text.splitlines(keepends=True):
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            section = stripped
            out.append(line)
            continue

        # `[package]` 段内：版本号必须等于目标。
        if section == "[package]":
            m = PKG_VERSION_RE.match(line.rstrip("\n"))
            if m and m.group(2) != target:
                changed.append(f"- {m.group(2)}\n+ {target}")
                line = f'{m.group(1)}{target}{m.group(3)}\n'
            out.append(line)
            continue

        # 依赖段内：只处理指向 workspace 成员的 path 依赖。
        m = DEP_LINE_RE.match(line.rstrip("\n"))
        if not m:
            out.append(line)
            continue
        indent, name, open_part, body, close_part = m.groups()
        if name not in internal:
            out.append(line)
            continue
        if not PATH_RE.search(body):
            out.append(line)
            continue
        vm = INNER_VERSION_RE.search(body)
        if not vm:
            errors.append(f"内部依赖 `{name}` 有 path 但缺 version 约束：{line.strip()}")
            out.append(line)
            continue
        if vm.group(2) == target:
            out.append(line)
            continue
        changed.append(f"- {name}: {vm.group(2)}\n+ {name}: {target}")
        body = body[: vm.start(2)] + target + body[vm.end(2) :]
        out.append(f"{indent}{name}{open_part}{body}{close_part}\n")

    return "".join(out), changed, errors


def run(root: Path, target: str, dry_run: bool) -> int:
    manifests = workspace_members(root)
    internal = member_names(manifests)
    total_files = 0
    total_changes = 0
    problems: list[str] = []

    for manifest in manifests:
        rel = manifest.relative_to(root)
        text = manifest.read_text(encoding="utf-8")
        new_text, changed, errors = rewrite(text, target, internal)
        problems.extend(f"{rel}: {e}" for e in errors)
        if errors:
            continue
        if changed:
            total_files += 1
            total_changes += len(changed)
            print(f"\n{rel}:")
            for c in changed:
                print(f"  {c}")
            if not dry_run:
                manifest.write_text(new_text, encoding="utf-8")

    print(
        f"\n{'（dry-run，未写入）' if dry_run else ''}"
        f"共 {total_files} 个 manifest、{total_changes} 处版本号"
        f"{'需要' if dry_run else '已'}更新为 {target}"
    )
    if problems:
        print("\n❌ 存在无法自动判定的条目：")
        for p in problems:
            print(f"   {p}")
        return 1
    return 0


def check(root: Path, target: str) -> int:
    """只校验：任何 manifest 与目标版本不一致即失败（不写文件）。"""
    manifests = workspace_members(root)
    internal = member_names(manifests)
    bad: list[str] = []
    for manifest in manifests:
        rel = manifest.relative_to(root)
        text = manifest.read_text(encoding="utf-8")
        _, changed, errors = rewrite(text, target, internal)
        bad.extend(f"{rel}: {e}" for e in errors)
        if changed:
            bad.append(f"{rel}: {len(changed)} 处仍不是 {target}")
    if bad:
        print(f"❌ 版本不一致（目标 {target}）：")
        for b in bad:
            print(f"   {b}")
        return 1
    print(f"✅ 全部 {len(manifests)} 个 manifest 的版本号均为 {target}")
    return 0


SAMPLE = """[package]
name = "demo-core"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = "1.0"
demo-util = { path = "../demo-util", version = "0.1.0" }
demo-nodecl = { path = "../demo-nodecl" }

[dev-dependencies]
serde_json = { version = "1.0", features = ["preserve_order"] }
"""


def self_check() -> int:
    """判别用例：能改的改对、不该改的不动、缺约束必须报错。

    这几条不是形式主义——bump 脚本最危险的失败模式是「把第三方依赖的版本
    当成自己的改掉」或「静默补上缺失的 version 掩盖作者本意」，两种都不会
    被编译期发现。
    """
    internal = {"demo-core", "demo-util", "demo-nodecl"}
    new, changed, errors = rewrite(SAMPLE, "0.2.0", internal)

    failures: list[str] = []
    if 'version = "0.2.0"' not in new.split("[dependencies]")[0]:
        failures.append("[package] version 未被更新")
    if 'demo-util = { path = "../demo-util", version = "0.2.0" }' not in new:
        failures.append("内部 path 依赖的 version 未被更新")
    if 'serde = "1.0"' not in new:
        failures.append("第三方依赖 serde 被误改")
    if 'serde_json = { version = "1.0", features = ["preserve_order"] }' not in new:
        failures.append("第三方依赖 serde_json 被误改")
    if len(errors) != 1 or "demo-nodecl" not in errors[0]:
        failures.append(f"缺 version 的内部依赖未被报出（errors={errors}）")
    if len(changed) != 2:
        failures.append(f"改动计数应为 2，实际 {len(changed)}")

    # 幂等：同一目标版本再跑一次不应产生任何改动。
    again, changed_again, errors_again = rewrite(new, "0.2.0", internal)
    if changed_again:
        failures.append("同一目标版本重复执行仍产生改动（非幂等）")
    if again != new:
        failures.append("幂等执行后文本发生变化")
    if len(errors_again) != 1:
        failures.append("幂等执行后错误集变化")

    # 目标版本不同 → 必须再次产生改动（否则「检查」永远是绿的）。
    bumped, changed_bumped, _ = rewrite(new, "0.3.0", internal)
    if len(changed_bumped) != 2 or 'version = "0.3.0"' not in bumped:
        failures.append("换目标版本后未产生改动")

    if failures:
        print("❌ 自检失败：")
        for f in failures:
            print(f"   {f}")
        return 2
    print("✅ 自检通过（5 个判别用例 + 2 项幂等/变更性检查）")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="统一 bump workspace 版本号")
    parser.add_argument("version", nargs="?", help="目标版本，如 0.3.0")
    parser.add_argument("--dry-run", action="store_true", help="只打印，不写文件")
    parser.add_argument("--check", action="store_true", help="只校验版本是否已一致")
    parser.add_argument("--self-check", action="store_true", help="跑改写逻辑的判别用例")
    args = parser.parse_args()

    if args.self_check:
        return self_check()

    if not args.version:
        parser.error("需要目标版本（或用 --self-check）")
    if not SEMVER_RE.match(args.version):
        parser.error(f"版本号格式非法：{args.version}")

    if args.check:
        return check(ROOT, args.version)
    return run(ROOT, args.version, args.dry_run)


if __name__ == "__main__":
    sys.exit(main())
