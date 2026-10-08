#!/usr/bin/env bash
# 逐 crate 跑 `cargo test-all` 的每个成员，跑到一个算一个，失败也继续。
#
# 存在的理由（都是实测撞过的坑，不是预防性设计）：
#
# 1. `cargo test --workspace` 是单次调用。实测 2026-10-08，`make test-all`
#    跑满 600s 被外部超时杀掉，期间完成了 24 个测试二进制，却因为
#    `tail -40` 缓冲只能看到最后几行 —— 一条结论都拿不出来。
#    拆成逐 crate 后，超时最多只影响当前那一个，且已完成的不用重跑。
#
# 2. 那次唯一失败的用例（cognitive_trace_persist_test::...survives_recreation）
#    在隔离复跑时只要 29.29s，在全量并发下要 306.47s。逐 crate 跑同时
#    也给出了“哪个 crate 对机器负载敏感”的信息。
#
# 退出码：只要有任何 crate 失败就返回 1，并列出失败清单。
# 用法：
#   bash scripts/run_test_chunks.sh            # 跳过已完成的
#   bash scripts/run_test_chunks.sh --reset    # 清空进度重来
#   CRATE_TIMEOUT=300 bash scripts/run_test_chunks.sh   # 单 crate 超时秒数

set -uo pipefail

PROGRESS=".testall-progress"
LOG=".testall-chunk.log"
# 单个 crate 的墙上时间上限。默认 600s：实测最慢的 core 在机器满载时
# 也要 300s 出头，600s 留了余量；再大就会让「卡死」和「慢」分不清。
CRATE_TIMEOUT="${CRATE_TIMEOUT:-600}"

cd "$(dirname "$0")/.." || exit 1

if [ "${1:-}" = "--reset" ]; then
    rm -f "$PROGRESS" "$LOG"
    echo "已重置进度"
fi

# ── 解析 test-all alias ──
# 刻意不硬编码 crate 名：硬编码的副本必然和 .cargo/config.toml 漂移，
# 而漂移的表现是「测试静默少跑几个 crate」——最难发现的一种失败。
#
# 直接读 .cargo/config.toml，而不是解析 `cargo --list`：
#   - `cargo --list` 的输出格式随 cargo 版本变（实测是
#     `    test-all    alias: test --workspace --exclude ...`，无引号）；
#   - .cargo/config.toml 是我们真正要保持同步的那份源。
# 用 python3 的 tomllib 直接读，拿不到就报错——绝不猜格式。
alias_def=$(python3 - <<'PY'
import sys
try:
    import tomllib
except ModuleNotFoundError:
    print("需要 Python 3.11+ 的 tomllib", file=sys.stderr)
    sys.exit(2)
with open(".cargo/config.toml", "rb") as f:
    cfg = tomllib.load(f)
alias = cfg.get("alias", {}).get("test-all")
if not alias:
    sys.exit(3)
# alias 可以是字符串，也可以是数组形式
if isinstance(alias, list):
    alias = " ".join(alias)
print(alias)
PY
)
rc=$?
if [ $rc -ne 0 ]; then
    echo "❌ 未能从 .cargo/config.toml 读到 [alias] test-all" >&2
    echo "   exit=$rc（2=缺 tomllib，3=alias 不存在或为空）" >&2
    exit 2
fi

mapfile -t CRATES < <(echo "$alias_def" | grep -o '\--exclude[= ][a-z0-9_-]*' | sed 's/--exclude[= ]//')

if [ "${#CRATES[@]}" -eq 0 ]; then
    echo "❌ 从 alias '$alias_def' 里没解析出任何 --exclude 项" >&2
    exit 2
fi

# 真正要测的 crate = workspace 成员 - 被排除的
mapfile -t ALL_PKGS < <(cargo metadata --format-version 1 --no-deps 2>/dev/null |
    python3 -c "
import json,sys
for p in sorted(json.load(sys.stdin)['packages'], key=lambda x: x['name']):
    print(p['name'])
")

# cargo metadata 失败时上面会静默产出空列表，TARGETS 也会空，
# 那样脚本会“全部通过”地骗人。必须挡住。
if [ "${#ALL_PKGS[@]}" -eq 0 ]; then
    echo "❌ cargo metadata 没返回任何 package（网络/锁文件问题？）" >&2
    echo "   此时若继续，会把 0 个 crate 当成“全部通过”" >&2
    exit 2
fi

TARGETS=()
for p in "${ALL_PKGS[@]}"; do
    skip=0
    for c in "${CRATES[@]}"; do
        [ "$p" = "$c" ] && skip=1 && break
    done
    [ "$skip" -eq 0 ] && TARGETS+=("$p")
done

# 与 alias 的一致性交叉检查：alias 说排 N 个，但算出来待测为 0 或全排光，
# 说明 alias 与 workspace 已经漂移。
if [ "${#TARGETS[@]}" -eq 0 ] || [ "${#TARGETS[@]}" -eq "${#ALL_PKGS[@]}" ]; then
    echo "❌ 过滤结果异常：workspace 共 ${#ALL_PKGS[@]} 个包，排除 ${#CRATES[@]} 个，待测 ${#TARGETS[@]} 个" >&2
    exit 2
fi

echo "alias      : $alias_def"
echo "排除        : ${CRATES[*]}"
echo "待测 crate  : ${#TARGETS[@]} 个"
echo "单 crate 上限: ${CRATE_TIMEOUT}s"
echo "日志        : $LOG"
echo ""

done_count=0
fail_count=0
failed_list=()

for pkg in "${TARGETS[@]}"; do
    if [ -f "$PROGRESS" ] && grep -qxF "$pkg" "$PROGRESS"; then
        echo "⏭️   $pkg（已通过，跳过）"
        done_count=$((done_count + 1))
        continue
    fi

    start=$(date +%s)
    printf '▶️  %-32s ' "$pkg"

    # 单 crate 独立超时：否则一个卡死的 crate 会拖死整轮。
    timeout "${CRATE_TIMEOUT}" \
        env CARGO_BUILD_JOBS=1 cargo test -p "$pkg" --no-fail-fast \
        > "$LOG.$pkg" 2>&1
    rc=$?
    elapsed=$(( $(date +%s) - start ))

    if [ $rc -eq 0 ]; then
        printf '✅ %ss\n' "$elapsed"
        echo "$pkg" >> "$PROGRESS"
        done_count=$((done_count + 1))
        rm -f "$LOG.$pkg"
    elif [ $rc -eq 124 ]; then
        printf '⏱️  %ss 超时（上限 %ss）\n' "$elapsed" "$CRATE_TIMEOUT"
        failed_list+=("$pkg (超时 ${elapsed}s)")
        fail_count=$((fail_count + 1))
    else
        printf '❌ %ss\n' "$elapsed"
        # 失败详情保留在日志里供查
        failed_list+=("$pkg (exit $rc)")
        fail_count=$((fail_count + 1))
        grep -E '^test result: FAILED|^error(\[|:)' "$LOG.$pkg" 2>/dev/null | head -5 | sed 's/^/      /'
    fi
done

echo ""
echo "━━━ 结果 ━━━"
echo "通过 $done_count / ${#TARGETS[@]}，失败 $fail_count"
if [ $fail_count -gt 0 ]; then
    echo ""
    echo "失败清单："
    for f in "${failed_list[@]}"; do
        echo "  - $f"
    done
    echo ""
    echo "失败 crate 的完整日志保留在 ${LOG}.*"
    exit 1
fi
echo "全部通过"
