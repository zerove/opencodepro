#!/usr/bin/env python3
"""审计「产品版本」在 CHANGELOG / manifest / 文档声明之间的一致性。

## 为什么需要这个门禁

版本号散落在 CHANGELOG、27 个 Cargo.toml、AGENTS.md 与 book 文档里，靠人眼
对账已多次漂移：book 停在 v0.3.0 而 CHANGELOG 已到 v0.4.0；AGENTS.md 的
crate 版本描述停留在 0.2.0。`bump_version.py --check` 只管 manifest 内部
一致性（`[package] version` 与 path 依赖约束），不校验文档声明；本脚本补上
「文档声明 ↔ CHANGELOG ↔ manifest」三方一致性。

以 CHANGELOG 最新版本为基准，校验：

1. 全部 manifest 的 `[package] version` 等于基准；
2. manifest 内指向 workspace 成员的 path 依赖带 `version` 约束且等于基准
   （与 `bump_version.py --check` 互为独立复核）；
3. AGENTS.md「全部 N 个 crate 当前都是 X.Y.Z」声明等于基准；
4. book 三处版本声明（introduction / getting-started / rest-api）等于基准。

不校验 docs/benchmarks 等历史档案里的版本（历史不追溯）。

用法：
    python3 scripts/audit_version.py               # 校验（CI / make audit）
    python3 scripts/audit_version.py --self-check  # 判据自检
    python3 scripts/audit_version.py --json
退出码：0 通过；1 违例；2 自检失败。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CHANGELOG_RE = re.compile(r"^## \[v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)\]", re.M)
AGENTS_RE = re.compile(r"全部 (\d+) 个 crate 当前都是 (\d+\.\d+\.\d+)")
ACTIVE_DEV_RE = re.compile(r"\*\*积极开发\*\*中（v?(\d+\.\d+\.\d+)）")
JSON_VER_RE = re.compile(r'"version"\s*:\s*"(\d+\.\d+\.\d+)"')

DEP_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")

MANIFESTS = sorted(ROOT.glob("crates/*/Cargo.toml")) + [
    ROOT / "webui/Cargo.toml",
    ROOT / "evals/Cargo.toml",
]

DOC_DECLARATIONS = [
    ("book/src/introduction.md", ACTIVE_DEV_RE),
    ("book/src/getting-started.md", JSON_VER_RE),
    ("book/src/rest-api.md", JSON_VER_RE),
]


def changelog_latest_version(root: Path) -> str | None:
    text = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    m = CHANGELOG_RE.search(text)
    return m.group(1) if m else None


def line_of(text: str, needle: str, start: int = 0) -> int:
    idx = text.find(needle, start)
    if idx < 0:
        return 0
    return text.count("\n", 0, idx) + 1


def check_manifest_text(text: str, expected: str, rel: str = "<memory>") -> list[dict]:
    """纯函数：校验单个 manifest 文本，供 --self-check 喂样本。"""
    problems: list[dict] = []
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as e:
        return [{"path": rel, "line": 0, "problem": f"TOML 解析失败: {e}"}]

    pkg = data.get("package", {})
    actual = pkg.get("version")
    if actual != expected:
        ln = line_of(text, 'version = "')
        problems.append({"path": rel, "line": ln, "problem": f"[package] version = {actual!r}，期望 {expected}"})

    if not re.search(r"^publish = false$", text, re.M):
        ln = line_of(text, "version = ")
        problems.append({"path": rel, "line": ln, "problem": "缺 publish = false（防误发）"})

    def tables(d: dict, prefix: str = ""):
        for key in DEP_TABLES:
            yield prefix + key, d.get(key, {})
        for tname, tval in d.get("target", {}).items():
            if isinstance(tval, dict):
                for key in DEP_TABLES:
                    yield f"{prefix}target.{tname}.{key}", tval.get(key, {})

    for table_name, deps in tables(data):
        for name, spec in deps.items():
            if not isinstance(spec, dict) or "path" not in spec:
                continue
            ver = spec.get("version")
            if ver != expected:
                ln = line_of(text, f"{name} =")
                problems.append(
                    {"path": rel, "line": ln,
                     "problem": f"path 依赖 {name}（{table_name}）version = {ver!r}，期望 {expected}"}
                )
    return problems


def check_text_declaration(text: str, pattern: re.Pattern[str], expected: str, rel: str) -> list[dict]:
    """纯函数：校验单处文档声明，供 --self-check 喂样本。"""
    m = pattern.search(text)
    if not m:
        return [{"path": rel, "line": 0, "problem": "未找到版本声明"}]
    actual = m.group(1)
    if actual != expected:
        ln = text.count("\n", 0, m.start()) + 1
        return [{"path": rel, "line": ln, "problem": f"声明版本 {actual}，期望 {expected}"}]
    return []


def audit_tree(root: Path) -> tuple[str | None, list[dict]]:
    baseline = changelog_latest_version(root)
    if baseline is None:
        return None, [{"path": "CHANGELOG.md", "line": 0, "problem": "未找到 `## [vX.Y.Z]` 版本条目"}]

    problems: list[dict] = []
    for manifest in MANIFESTS:
        rel = manifest.relative_to(root).as_posix()
        problems.extend(check_manifest_text(manifest.read_text(encoding="utf-8"), baseline, rel))

    agents = (root / "AGENTS.md").read_text(encoding="utf-8")
    m = AGENTS_RE.search(agents)
    if not m:
        problems.append({"path": "AGENTS.md", "line": 0,
                         "problem": "未找到「全部 N 个 crate 当前都是 X.Y.Z」声明"})
    else:
        count, declared = m.group(1), m.group(2)
        ln = agents.count("\n", 0, m.start()) + 1
        if declared != baseline:
            problems.append({"path": "AGENTS.md", "line": ln, "problem": f"声明 crate 版本 {declared}，基准 {baseline}"})
        if int(count) != len(MANIFESTS):
            problems.append({"path": "AGENTS.md", "line": ln,
                             "problem": f"声明 crate 数 {count}，实际 manifest 数 {len(MANIFESTS)}"})

    for rel_path, pattern in DOC_DECLARATIONS:
        rel = rel_path
        problems.extend(check_text_declaration(
            (root / rel_path).read_text(encoding="utf-8"), pattern, baseline, rel,
        ))

    return baseline, problems


def self_check() -> int:
    ok = True

    good = (
        '[package]\nname = "x"\nversion = "1.2.3"\npublish = false\n\n'
        '[dependencies]\nsisyphus-types = { path = "../types", version = "1.2.3" }\n'
    )
    if check_manifest_text(good, "1.2.3"):
        print("自检失败：合法 manifest 被误报", file=sys.stderr); ok = False

    bad_version = '[package]\nname = "x"\nversion = "9.9.9"\npublish = false\n'
    hits = check_manifest_text(bad_version, "1.2.3")
    if len(hits) != 1 or "version = '9.9.9'" not in hits[0]["problem"]:
        print("自检失败：version 漂移未拦截", file=sys.stderr); ok = False

    bad_dep_missing = '[package]\nname = "x"\nversion = "1.2.3"\npublish = false\n\n[dependencies]\nfoo = { path = "../foo" }\n'
    hits = check_manifest_text(bad_dep_missing, "1.2.3")
    if len(hits) != 1 or "foo" not in hits[0]["problem"]:
        print("自检失败：path 依赖缺 version 未拦截", file=sys.stderr); ok = False

    bad_dep_mismatch = '[package]\nname = "x"\nversion = "1.2.3"\npublish = false\n\n[dependencies]\nfoo = { path = "../foo", version = "0.1.0" }\n'
    hits = check_manifest_text(bad_dep_mismatch, "1.2.3")
    if len(hits) != 1 or "0.1.0" not in hits[0]["problem"]:
        print("自检失败：path 依赖版本不符未拦截", file=sys.stderr); ok = False

    bad_publish = '[package]\nname = "x"\nversion = "1.2.3"\n\n[dependencies]\n'
    hits = check_manifest_text(bad_publish, "1.2.3")
    if len(hits) != 1 or "publish" not in hits[0]["problem"]:
        print("自检失败：缺 publish = false 未拦截", file=sys.stderr); ok = False

    if check_text_declaration("处于**积极开发**中（v1.2.3）。", ACTIVE_DEV_RE, "1.2.3", "t"):
        print("自检失败：合法文档声明被误报", file=sys.stderr); ok = False
    hits = check_text_declaration("处于**积极开发**中（v1.1.0）。", ACTIVE_DEV_RE, "1.2.3", "t")
    if len(hits) != 1:
        print("自检失败：文档声明漂移未拦截", file=sys.stderr); ok = False

    if ok:
        print("✅ 判据自检通过（manifest version / publish = false / path 依赖约束 / 文档声明 各 2 例）")
        return 0
    return 2


def main() -> int:
    parser = argparse.ArgumentParser(description="版本一致性门禁")
    parser.add_argument("--self-check", action="store_true", help="判据自检")
    parser.add_argument("--json", action="store_true", help="JSON 输出")
    args = parser.parse_args()

    if args.self_check:
        return self_check()

    baseline, problems = audit_tree(ROOT)
    if args.json:
        print(json.dumps({"baseline": baseline, "manifests": len(MANIFESTS), "problems": problems},
                         ensure_ascii=False, indent=2))
    else:
        for p in problems:
            print("%s:%d: %s" % (p["path"], p["line"], p["problem"]))
        if problems:
            print("\n❌ %d 处版本不一致（基准 v%s）。改 manifest 用 scripts/bump_version.py，"
                  "再同步 AGENTS.md 与 book 声明。" % (len(problems), baseline), file=sys.stderr)
        else:
            print("✅ 版本一致性通过（基准 v%s；%d 个 manifest + AGENTS.md + book 三处声明）"
                  % (baseline, len(MANIFESTS)))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
