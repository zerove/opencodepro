#!/usr/bin/env python3
"""审计 Docker 发布流水线承诺的「镜像名」与「tag」是否真的会被产出。

## 为什么需要这层门禁

发布流水线的错误**不会在执行前暴露**，而且大多只在用户机器上表现为「拉不到镜像」：

1. **镜像名**：`docker-release.yml` 与 `nightly.yml` 原本都写
   `IMAGE_NAME: ${{ github.repository }}`。它在 `zerove/opencodepro` 仓库下展开成
   `ghcr.io/zerove/opencodepro`，而 README、`book/src/deployment.md`、
   `docker-compose.yml` 里每一处 `docker pull` 写的都是
   `ghcr.io/zerove/sisyphus-bridge`。两边都「看起来对」——一个是仓库名、
   一个是产品名——但推上去和拉下来的根本不是同一个包。
2. **tag 形式**：文档承诺 `:v0.3.0` 与 `:latest`。可 `type=match,...,group=1`
   产出的是**去掉 v 前缀**的 `0.3.0`（要 `:v0.3.0` 必须额外写
   `type=raw,value={{tag}}`），而 `flavor: latest=false` 意味着 `:latest`
   永远不会存在。用户照着文档 `docker pull ...:latest` 必然 404。
3. **`:latest` 归属**：nightly 与 release 都推 `:latest` 时，用户拿到的到底是
   最近一次稳定版还是某个 nightly 快照，取决于哪条流水线最后跑完——同一 tag
   指向一个每晚都在变的镜像。

三类缺陷都不会让 CI 变红，也不会让任何 Rust 测试失败；它们只在「真的去拉镜像」
那一刻出现。所以判据必须是**算出来的**，而不是读注释：本脚本按
docker/metadata-action 的语义把 tag 集合算出来，再和文档承诺、compose 引用的
镜像名逐条比对。

## 判据

1. 各 workflow 的 `env.IMAGE_NAME` 必须是字面量（不能是 `${{ ... }}`），且与
   `docker-compose.yml`、`README.md`、`book/src/deployment.md` 里的
   `ghcr.io/<owner>/<name>` 一致；
2. 以当前 workspace 版本构造稳定 tag（`vX.Y.Z`），算出的 tag 集合必须覆盖
   `:vX.Y.Z` / `:X.Y.Z` / `:X.Y` / `:X` / `:latest`；
3. 预发布 tag（`vX.Y.Z-rc.1`）不得产出 `:latest`；
4. 除 `docker-release.yml` 外，任何 workflow 都不得产出 `latest`（同一镜像的
   可变 tag 只能有一个主人）。

## 用法

    python3 scripts/audit_docker_release.py
    python3 scripts/audit_docker_release.py --self-check

退出码：0 通过 / 1 发现问题 / 2 脚本或输入错误。
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
RELEASE_WF = ".github/workflows/docker-release.yml"
COMPOSE = "docker-compose.yml"
DOCS = ("README.md", "book/src/deployment.md")


def all_workflows() -> list[str]:
    """全部 workflow（相对仓库根）。

    不写死文件清单：新增一条推镜像的流水线时，门禁必须自动覆盖它，而不是
    因为「名单里没有」而静默跳过。
    """
    return sorted(
        str(p.relative_to(ROOT)) for p in (ROOT / ".github/workflows").glob("*.yml")
    )

SEMVER_RE = re.compile(r"^v?(?P<major>\d+)\.(?P<minor>\d+)\.(?P<patch>\d+)(?:-(?P<pre>[0-9A-Za-z.-]+))?$")


# ── metadata-action 语义的最小实现 ──────────────────────────────────────
#
# 只覆盖本仓库用到的类型（raw / match / semver / sha / schedule）与
# flavor 的 latest/prefix/suffix。遇到没见过的写法**直接报错**：静默忽略会让
# 一次「新增了一条 tag 规则」变成「门禁悄悄少验一条」。


class UnsupportedSpec(Exception):
    """遇到了本脚本尚未建模的 metadata-action 写法。"""


def _split_parts(line: str) -> list[str]:
    """按顶层逗号切分一行规则。

    不能直接 `line.split(",")`：`enable=${{ startsWith(github.ref,
    'refs/tags/') }}` 本身就带逗号，切错了会把表达式后半段当成一条新规则，
    于是 enable 恒为真、门禁刚好在最需要它的时候失效。
    """
    parts: list[str] = []
    buf: list[str] = []
    depth = 0
    quote = ""
    i = 0
    while i < len(line):
        ch = line[i]
        if quote:
            buf.append(ch)
            if ch == quote:
                quote = ""
        elif ch in "'\"":
            quote = ch
            buf.append(ch)
        elif line.startswith("{{", i):
            depth += 1
            buf.append("{{")
            i += 1
        elif line.startswith("}}", i) and depth:
            depth -= 1
            buf.append("}}")
            i += 1
        elif ch == "," and depth == 0:
            parts.append("".join(buf))
            buf = []
        else:
            buf.append(ch)
        i += 1
    if buf:
        parts.append("".join(buf))
    return [p for p in (p.strip() for p in parts) if p]


def _entries(spec: str) -> list[dict[str, str]]:
    """把 `tags: |` 块解析成若干键值对（`#` 注释行剔除）。"""
    out: list[dict[str, str]] = []
    for raw_line in spec.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        entry: dict[str, str] = {}
        for part in _split_parts(line):
            key, sep, value = part.partition("=")
            if not sep:
                raise UnsupportedSpec(f"无法解析的 tag 规则片段：{part!r}")
            entry[key.strip()] = value.strip()
        out.append(entry)
    return out


def _expand(template: str, ctx: dict[str, str]) -> str:
    """展开 `{{major}}` / `{{date 'YYYYMMDD'}}` 这类模板。未知模板直接报错。"""

    def repl(m: re.Match[str]) -> str:
        key = m.group(1).strip()
        date_m = re.fullmatch(r"date\s+'([^']+)'", key)
        if date_m:
            # 固定假日期：本脚本只关心 tag **形状**，不关心具体是哪天。
            fake = {"YYYY": "2026", "MM": "01", "DD": "01"}
            return date_m.group(1).replace("YYYY", fake["YYYY"]).replace("MM", fake["MM"]).replace("DD", fake["DD"])
        if key not in ctx:
            raise UnsupportedSpec(f"未知模板 {{{{ {key} }}}}（已知：{sorted(ctx)}）")
        return ctx[key]

    return re.sub(r"\{\{(.*?)\}\}", repl, template)


def _enabled(expr: str | None, event: str) -> bool:
    """`enable=` 的最小求值。

    只支持本仓库实际用到的两种：`startsWith(github.ref, 'refs/tags/')` 与
    `{{is_default_branch}}`。其它表达式抛错，避免用「默认 True」蒙混过去。
    """
    if not expr:
        return True
    expr = expr.strip()
    if expr == "true":
        return True
    if expr == "false":
        return False
    if "startsWith(github.ref, 'refs/tags/')" in expr:
        return event == "tag"
    if "is_default_branch" in expr:
        return event in ("branch", "schedule", "push")
    raise UnsupportedSpec(f"未知 enable 表达式：{expr}")


def _flavor(spec: str | None) -> dict[str, str]:
    out: dict[str, str] = {}
    for raw_line in (spec or "").splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        # `prefix=foo,onlatest=true` 是合法写法：值里带逗号，按顶层逗号切。
        for part in _split_parts(line):
            key, sep, value = part.partition("=")
            if not sep:
                raise UnsupportedSpec(f"无法解析的 flavor 片段：{part!r}")
            out[key.strip()] = value.strip()
    unknown = set(out) - {"latest", "prefix", "suffix", "onlatest"}
    if unknown:
        raise UnsupportedSpec(f"未建模的 flavor 键：{sorted(unknown)}")
    return out


def eval_tags(tags_spec: str, flavor_spec: str | None, ref: str, event: str = "tag") -> set[str]:
    """算出某个 ref 在 metadata-action 下会产出哪些 tag。

    `ref` 是 ref 名（tag 名如 `v0.3.0`，或分支名如 `main`）；`event` 取
    `tag` / `branch` / `schedule`。语义依据 metadata-action v6 文档：
    - `{{tag}}` 只在 tag ref 下非空，`{{branch}}` 只在分支 ref 下非空；
    - **预发布 semver**（如 `v0.3.0-rc.1`）只产出 `{{version}}`/`{{raw}}`，
      `{{major}}` 这类模式不会产出 `0`——否则一个 rc 就会把 `:0` 从稳定版
      抢走；
    - `flavor latest=auto` 只对非预发布 semver 生效。
    """
    flavor = _flavor(flavor_spec)
    sem = SEMVER_RE.match(ref) if event == "tag" else None
    version = ""
    ctx: dict[str, str] = {"tag": "", "branch": "", "sha": "0" * 40}
    if event == "tag":
        ctx["tag"] = ref
    if event == "branch":
        ctx["branch"] = ref
    if sem:
        version = (
            f"{sem.group('major')}.{sem.group('minor')}.{sem.group('patch')}"
            + (f"-{sem.group('pre')}" if sem.group("pre") else "")
        )
        ctx.update(
            {
                "version": version,
                "raw": ref,
                "major": sem.group("major"),
                "minor": sem.group("minor"),
                "patch": sem.group("patch"),
            }
        )

    produced: set[str] = set()
    for entry in _entries(tags_spec):
        if not _enabled(entry.get("enable"), event):
            continue
        kind = entry.get("type")
        if kind == "raw":
            value = _expand(entry.get("value", ""), ctx)
            if value:
                produced.add(value)
        elif kind == "match":
            pattern = entry.get("pattern")
            if not pattern:
                raise UnsupportedSpec("match 规则缺少 pattern")
            group = int(entry.get("group", "0"))
            m = re.search(pattern, ref) if event == "tag" else None
            if m:
                produced.add(m.group(group))
        elif kind == "semver":
            if not sem:
                continue
            pattern = entry.get("pattern", "")
            if sem.group("pre") and "{{raw}}" not in pattern:
                produced.add(version)
            else:
                produced.add(_expand(pattern, ctx))
        elif kind == "sha":
            produced.add(f"{entry.get('prefix', '')}{ctx['sha'][:7]}")
        elif kind == "schedule":
            if event == "schedule":
                produced.add(_expand(entry.get("pattern", ""), {**ctx, "date": "2026-01-01"}))
        else:
            raise UnsupportedSpec(f"未建模的 tag 类型：{kind!r}")

    prefix = flavor.get("prefix", "")
    suffix = flavor.get("suffix", "")
    if prefix or suffix:
        produced = {f"{prefix}{t}{suffix}" for t in produced}

    latest_mode = flavor.get("latest", "auto")
    if latest_mode not in {"auto", "true", "false"}:
        raise UnsupportedSpec(f"未建模的 latest 取值：{latest_mode!r}")
    if latest_mode == "true" or (latest_mode == "auto" and sem and not sem.group("pre")):
        produced.add("latest")

    return produced


# ── 仓库事实的读取 ─────────────────────────────────────────────────────


def load_workflow(rel: str) -> dict:
    return yaml.safe_load((ROOT / rel).read_text(encoding="utf-8"))


def metadata_step(workflow: dict) -> dict:
    """找到 `docker/metadata-action` 的 step，返回其 `with`。"""
    for job in workflow.get("jobs", {}).values():
        for step in job.get("steps", []):
            if str(step.get("uses", "")).startswith("docker/metadata-action"):
                return step.get("with", {})
    raise LookupError("workflow 里没有 docker/metadata-action step")


def image_parts(raw: str) -> tuple[str, str]:
    """把 `IMAGE_NAME` 拆成 (owner, 产品名)。

    合法写法只有两种：
    - `<owner>/<name>`（字面量 owner）
    - `${{ github.repository_owner }}/<name>`（fork 后仍推自己的 registry）

    裸 `<name>` **不合法**：ghcr 的路径是 `ghcr.io/<owner>/<name>`，只有产品名
    会拼出 `ghcr.io/sisyphus-bridge`，push 直接失败。用
    `${{ github.repository }}` 当镜像名同样不行——它展开成仓库名，而不是产品名。
    """
    if "${{" in raw:
        m = re.fullmatch(r"\$\{\{\s*github\.repository_owner\s*\}\}/([A-Za-z0-9._-]+)", raw)
        if not m:
            raise ValueError(
                f"IMAGE_NAME={raw!r} 不是受支持的写法：owner 位置只允许 "
                "`${{ github.repository_owner }}`，且必须带 `/` 分隔的产品名"
            )
        return "${github.repository_owner}", m.group(1)
    m = re.fullmatch(r"([A-Za-z0-9_-]+)/([A-Za-z0-9._-]+)", raw)
    if not m:
        raise ValueError(
            f"IMAGE_NAME={raw!r} 不是 `<owner>/<name>` 形式：ghcr 镜像路径是 "
            "`ghcr.io/<owner>/<name>`，裸产品名或带 registry 前缀都会拼错"
        )
    return m.group(1), m.group(2)


def workspace_version() -> str:
    versions = set()
    meta = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    for member in meta["workspace"]["members"]:
        pkg = tomllib.loads((ROOT / member / "Cargo.toml").read_text(encoding="utf-8"))["package"]
        versions.add(pkg["version"])
    if len(versions) != 1:
        raise ValueError(f"workspace 版本不一致：{sorted(versions)}")
    return versions.pop()


def documented_images() -> tuple[set[str], set[str]]:
    """从 compose 与文档里抽出 `ghcr.io/<owner>/<name>` 的 (owners, names)。"""
    owners: set[str] = set()
    names: set[str] = set()
    compose = (ROOT / COMPOSE).read_text(encoding="utf-8")
    for m in re.finditer(r"ghcr\.io/\$\{[^}:]+:-([A-Za-z0-9._-]+)\}/([A-Za-z0-9._-]+)", compose):
        owners.add(m.group(1))
        names.add(m.group(2))
    for rel in DOCS:
        text = (ROOT / rel).read_text(encoding="utf-8")
        for m in re.finditer(r"ghcr\.io/([A-Za-z0-9._-]+)/([A-Za-z0-9._-]+)", text):
            owners.add(m.group(1))
            names.add(m.group(2))
    return owners, names


# ── 检查项 ─────────────────────────────────────────────────────────────


def check_image_name() -> list[str]:
    problems: list[str] = []
    parts = {}
    for rel in all_workflows():
        raw = load_workflow(rel).get("env", {}).get("IMAGE_NAME")
        if raw:
            parts[rel] = image_parts(str(raw))
    if RELEASE_WF not in parts:
        problems.append(f"{RELEASE_WF} 未声明 env.IMAGE_NAME——发布链路没有可校验的目标")
    products = {name for _, name in parts.values()}
    if len(products) != 1:
        problems.append(f"workflow 之间的产品名不一致：{parts}")

    owners, documented_names = documented_images()
    if documented_names != products:
        problems.append(
            f"文档/compose 引用的产品名 {sorted(documented_names)} 与 workflow 推送的 "
            f"{sorted(products)} 不一致——照文档 pull 会 404"
        )
    for rel, (owner, _) in parts.items():
        if owner.startswith("${"):
            continue
        if owner not in owners:
            problems.append(
                f"{rel} 写死的 owner `{owner}` 与文档/compose 里的 {sorted(owners)} 不一致"
            )
    return problems


def check_release_tags() -> list[str]:
    problems: list[str] = []
    wf = load_workflow(RELEASE_WF)
    step = metadata_step(wf)
    version = workspace_version()
    stable = eval_tags(step.get("tags", ""), step.get("flavor"), f"v{version}")
    prerelease = eval_tags(step.get("tags", ""), step.get("flavor"), f"v{version}-rc.1")

    major, minor = version.split(".")[:2]
    must = {f"v{version}", version, f"{major}.{minor}", major, "latest"}
    missing = sorted(must - stable)
    if missing:
        problems.append(
            f"稳定版 v{version} 不会产出文档承诺的 tag：{missing}（实得 {sorted(stable)}）"
        )
    if "latest" in prerelease:
        problems.append(f"预发布 v{version}-rc.1 会打 :latest（实得 {sorted(prerelease)}）")
    for expected in (f"v{version}-rc.1", f"{version}-rc.1"):
        if expected not in prerelease:
            problems.append(f"预发布 v{version}-rc.1 缺少 tag {expected}（实得 {sorted(prerelease)}）")
    return problems


def check_latest_owner() -> list[str]:
    """`:latest` 只能由 release 流水线产出。"""
    problems: list[str] = []
    for rel in all_workflows():
        if rel == RELEASE_WF:
            continue
        wf = load_workflow(rel)
        if "jobs" not in wf:
            continue
        try:
            step = metadata_step(wf)
        except LookupError:
            continue
        # nightly 由 schedule / workflow_dispatch 触发，取 schedule 事件求值。
        produced = eval_tags(step.get("tags", ""), step.get("flavor"), "main", "schedule")
        if "latest" in produced:
            problems.append(
                f"{rel} 也会推 :latest（{sorted(produced)}）——与稳定发布抢同一个可变 tag"
            )
    return problems


CHECKS = (
    ("镜像名一致性", check_image_name),
    ("发布 tag 覆盖文档承诺", check_release_tags),
    (":latest 归属唯一", check_latest_owner),
)


# ── 自检：用判别用例钉住上面的语义模型 ─────────────────────────────────

STABLE_SPEC = """
    type=raw,value={{tag}},enable=${{ startsWith(github.ref, 'refs/tags/') }}
    type=match,pattern=^v(\\d+\\.\\d+)\\.0$,group=1
    type=match,pattern=^v(.+)$,group=1
    type=semver,pattern={{version}}
    type=semver,pattern={{major}}.{{minor}}
    type=semver,pattern={{major}}
"""


def self_check() -> int:
    failures: list[str] = []

    def expect(cond: bool, msg: str) -> None:
        if not cond:
            failures.append(msg)

    # 1. 当前配置的语义：稳定版覆盖全部形式，预发布不带 latest，也不抢 :0。
    stable = eval_tags(STABLE_SPEC, "latest=auto", "v0.3.0")
    expect(
        stable == {"v0.3.0", "0.3.0", "0.3", "0", "latest"},
        f"稳定版 tag 集合不符：{sorted(stable)}",
    )
    pre = eval_tags(STABLE_SPEC, "latest=auto", "v0.3.0-rc.1")
    expect(
        pre == {"v0.3.0-rc.1", "0.3.0-rc.1"},
        f"预发布 tag 集合不符（不应产出 0 / 0.3 / latest）：{sorted(pre)}",
    )

    # 2. latest=false 必须被识别成「不产出 latest」——修复前正是这个状态。
    expect("latest" not in eval_tags(STABLE_SPEC, "latest=false", "v0.3.0"), "latest=false 未被识别")

    # 3. match 的 group 语义：group=1 去前缀，group=0 保留原样。
    group0 = eval_tags("type=match,pattern=^v(.+)$,group=0", "latest=false", "v0.3.0")
    expect(group0 == {"v0.3.0"}, f"group=0 语义错误：{sorted(group0)}")

    # 4. 分支事件：match/semver 都不该命中，`{{tag}}` 为空 → 零 tag。
    expect(
        eval_tags(STABLE_SPEC, "latest=auto", "main", "branch") == set(),
        "分支事件仍产出了 tag",
    )

    # 5. nightly 风格：schedule 事件产出 nightly 系列，且不得含 latest。
    nightly = """
        type=schedule,pattern=nightly-{{date 'YYYYMMDD'}}
        type=raw,value=nightly
        type=sha,prefix=nightly-
    """
    got = eval_tags(nightly, None, "main", "schedule")
    expect(
        got == {"nightly", "nightly-20260101", "nightly-0000000"},
        f"schedule 语义错误：{sorted(got)}",
    )
    expect("latest" not in got, f"nightly 配置产出了 latest：{sorted(got)}")

    # 6. nightly 若写了 latest，必须被 ':latest 归属' 检查捕获。
    poisoned = nightly + "type=raw,value=latest,enable={{is_default_branch}}"
    expect(
        "latest" in eval_tags(poisoned, None, "main", "schedule"),
        "nightly 里的 :latest 未被识别",
    )

    # 7. 镜像名写法：必须带 owner，且不接受 `github.repository`。
    for bad in ("${{ github.repository }}", "sisyphus-bridge", "ghcr.io/zerove/sisyphus-bridge"):
        try:
            image_parts(bad)
            failures.append(f"非法 IMAGE_NAME 未被拒绝：{bad}")
        except ValueError:
            pass
    expect(
        image_parts("zerove/sisyphus-bridge") == ("zerove", "sisyphus-bridge"),
        "字面量 owner 拆分错误",
    )
    expect(
        image_parts("${{ github.repository_owner }}/sisyphus-bridge")
        == ("${github.repository_owner}", "sisyphus-bridge"),
        "owner 表达式拆分错误",
    )

    # 8. 未知写法不能静默放过。
    for spec, why in (
        ("type=edge,branch=main", "未知 tag 类型"),
        ("type=match,pattern=^v(.+)$,group=1,enable=${{ github.ref == 'refs/tags/x' }}", "未知 enable"),
        ("type=raw,value={{nonsense}}", "未知模板"),
    ):
        try:
            eval_tags(spec, None, "v0.3.0")
            failures.append(f"{why} 未报错：{spec}")
        except (UnsupportedSpec, KeyError):
            pass

    if failures:
        print("❌ 自检失败：")
        for f in failures:
            print(f"   {f}")
        return 2
    print("✅ 自检通过（8 个判别用例）")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="审计 Docker 发布流水线的镜像名与 tag 契约")
    parser.add_argument("--self-check", action="store_true", help="跑语义模型的判别用例")
    args = parser.parse_args()

    if args.self_check:
        return self_check()

    total = 0
    for name, check in CHECKS:
        try:
            problems = check()
        except (UnsupportedSpec, LookupError, ValueError) as e:
            print(f"❌ {name}: {e}")
            total += 1
            continue
        if problems:
            print(f"❌ {name}:")
            for p in problems:
                print(f"   {p}")
            total += len(problems)
        else:
            print(f"✅ {name}")

    if total:
        print(f"\n❌ Docker 发布契约审计失败：{total} 项")
        return 1
    print("\n✅ Docker 发布契约审计通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
