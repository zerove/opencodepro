#!/usr/bin/env bash
# 发布前置检查（release pre-flight）
#
# 目标：在 `git tag v*` 之前，用一条命令确认「仓库自身」处于可发布状态。
# 只做**可本地验证**的检查；真正触发 release.yml 的 `git tag` + `git push`
# 由人执行（脚本只提示，不代劳）。
#
# 检查项：
#   1. 全部 workspace crate 版本一致（单一版本号）
#   2. 全部 crate 具备发布必需 metadata（description / authors / license / repository）
#   3. CHANGELOG.md 存在 `## [Unreleased]` 段，且该段至少有一条 bullet
#   4. CHANGELOG.md 存在与当前版本对应的 `## [vX.Y.Z]`（或 `## [X.Y.Z]`）段落
#   5. 当前版本尚未被打过 tag（打过 → 说明需要先 bump 版本）
#   6. 工作区干净（未提交改动会被 `cargo package` 拒绝）
#   7. cargo metadata 可解析（manifest 语法有效）
#
# 已知限制（非本脚本可解）：内部 crate 未发布到 crates.io，`cargo package`
# 会因 `sisyphus-bridge-api` / `sisyphus-metrics` 等无法解析而失败。
# 因此本脚本用 `cargo package --list --allow-dirty` 之外的 metadata 检查替代，
# 不把该限制当作失败项。
#
# 用法：scripts/release-check.sh [--version X.Y.Z]
set -uo pipefail

VERSION_OVERRIDE=""
RETAG=0
while [ $# -gt 0 ]; do
    case "$1" in
        --retag)
            # 上次发布失败（Release workflow 没产出任何 artifact）时，允许重打
            # 同名 tag 而不 bump 版本。默认关闭：tag 一旦被消费就不该被移动。
            RETAG=1
            shift
            ;;
        --version)
            VERSION_OVERRIDE="${2:-}"
            [ -n "$VERSION_OVERRIDE" ] || {
                echo "error: --version 需要参数" >&2
                exit 2
            }
            shift 2
            ;;
        -h | --help)
            sed -n '2,25p' "$0"
            exit 0
            ;;
        *)
            echo "error: 未知参数 $1" >&2
            exit 2
            ;;
    esac
done

cd "$(dirname "$0")/.." || exit 1

FAIL=0
pass() { echo "  ✅ $1"; }
warn() { echo "  ⚠️  $1"; }
fail() {
    echo "  ❌ $1"
    FAIL=$((FAIL + 1))
}

echo "━━━ release pre-flight ━━━"

# ── 7. manifest 可解析 ────────────────────────────────
META_TMP="$(mktemp)"
trap 'rm -f "$META_TMP"' EXIT
if cargo metadata --no-deps --format-version 1 >"$META_TMP" 2>/dev/null; then
    pass "cargo metadata 解析成功"
else
    fail "cargo metadata 解析失败（manifest 语法错误？）"
    echo "release-check: 中止（manifest 无法解析）" >&2
    exit 1
fi

# ── 1. 版本一致性 ─────────────────────────────────────
read -r VERSIONS VERSION <<<"$(python3 - "$META_TMP" <<'PY'
import json, sys
from collections import Counter
d = json.load(open(sys.argv[1]))
c = Counter(p["version"] for p in d["packages"])
vers = ",".join(sorted(c))
top = c.most_common(1)[0][0]
print(vers, top)
PY
)"

if [ -n "$VERSION_OVERRIDE" ] && [ "$VERSION_OVERRIDE" != "$VERSION" ]; then
    fail "crate 版本为 $VERSION，但 --version 指定 $VERSION_OVERRIDE（需先 bump 版本）"
    # 后续检查针对**目标版本**做校验：CHANGELOG 是否已切段、tag 是否已存在
    VERSION="$VERSION_OVERRIDE"
elif [ "${VERSIONS}" = "$VERSION" ]; then
    pass "全部 crate 版本一致：$VERSION"
else
    fail "crate 版本不一致：$VERSIONS"
fi

# ── 2. 发布 metadata ─────────────────────────────────
MISSING="$(python3 - "$META_TMP" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
fields = ("description", "authors", "license", "repository")
bad = []
for p in d["packages"]:
    lack = [f for f in fields if not p.get(f)]
    if lack:
        bad.append(f"{p['name']}({'+'.join(lack)})")
print(" ".join(bad))
PY
)"
if [ -z "$MISSING" ]; then
    pass "全部 crate 具备 description/authors/license/repository"
else
    fail "缺少发布 metadata：$MISSING"
fi

# ── 3/4. CHANGELOG ───────────────────────────────────
if [ ! -f CHANGELOG.md ]; then
    fail "缺少 CHANGELOG.md"
else
    UNRELEASED_BULLETS="$(python3 - CHANGELOG.md <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
m = re.search(r"^##\s*\[Unreleased\]\s*$", text, re.M)
if not m:
    print(-1)
    sys.exit()
tail = text[m.end():]
nxt = re.search(r"^##\s", tail, re.M)
body = tail[: nxt.start()] if nxt else tail
print(len(re.findall(r"^\s*-\s+\S", body, re.M)))
PY
)"
    case "$UNRELEASED_BULLETS" in
        -1) fail "CHANGELOG.md 缺少 '## [Unreleased]' 段" ;;
        # 刚把 Unreleased 切成版本段时该段理应为空，不算失败
        0) warn "CHANGELOG.md 的 [Unreleased] 段为空（刚切完版本时属正常）" ;;
        *) pass "CHANGELOG.md [Unreleased] 段有 $UNRELEASED_BULLETS 条记录" ;;
    esac

    # 版本段匹配必须忽略 fenced code block —— 本文件的「发布流程」章节内嵌了
    # ```markdown 模板骨架，其中含有 `## [v0.2.0] - 2026-XX-XX` 字样，直接 grep 会误命中。
    SECTION_FOUND="$(python3 - CHANGELOG.md "$VERSION" <<'PY'
import re, sys

text = open(sys.argv[1], encoding="utf-8").read()
version = sys.argv[2]

# 去掉 ``` / ~~~ 围栏代码块（含未闭合的尾部），只留正文
out, fence = [], None
for line in text.splitlines():
    stripped = line.lstrip()
    if fence is None:
        m = re.match(r"^(```+|~~~+)", stripped)
        if m:
            fence = m.group(1)[0]
            continue
        out.append(line)
    else:
        if stripped.startswith(fence * 3):
            fence = None
    # 围栏内的一切内容都不保留
body = "\n".join(out)

hit = re.search(rf"^##\s*\[v?{re.escape(version)}\]", body, re.M)
print("yes" if hit else "no")
PY
)"
    if [ "$SECTION_FOUND" = "yes" ]; then
        pass "CHANGELOG.md 存在版本段 [$VERSION]"
    else
        fail "CHANGELOG.md 缺少版本段 [$VERSION]（应把 [Unreleased] 改名为 [v${VERSION}] - <日期>）"
    fi
fi

# ── 5. 版本是否已打 tag ──────────────────────────────
if git rev-parse --git-dir >/dev/null 2>&1; then
    if git rev-parse -q --verify "refs/tags/v${VERSION}" >/dev/null; then
        if [ "$RETAG" -eq 1 ]; then
            warn "tag v${VERSION} 已存在；--retag 允许重打（确认上次发布未产出 artifact）"
        else
            fail "tag v${VERSION} 已存在 → 需先 bump 版本；若上次发布失败且无产物，加 --retag"
        fi
    else
        pass "tag v${VERSION} 尚未创建（可直接 git tag）"
    fi

    # ── 6. 工作区干净 ────────────────────────────────
    if [ -z "$(git status --porcelain)" ]; then
        pass "工作区干净"
    else
        warn "工作区有未提交改动（cargo package 会拒绝，需要 --allow-dirty）"
    fi
else
    warn "不在 git 仓库中，跳过 tag / 工作区检查"
fi

echo "━━━ 结果 ━━━"
if [ "$FAIL" -eq 0 ]; then
    echo "✅ release pre-flight 通过（版本 $VERSION）"
    echo "   下一步：git tag -a v${VERSION} -m \"Release v${VERSION}\" && git push origin v${VERSION}"
    exit 0
fi
echo "❌ release pre-flight 失败：$FAIL 项" >&2
exit 1
