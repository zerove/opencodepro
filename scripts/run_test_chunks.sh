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
# 3. 记录必须由脱离父进程的进程来做。实测踩过：本脚本被 600s 外部超时
#    杀掉时，正在跑的 crate 作为孤儿进程跑完并通过，但父 shell 一死就没人
#    写耗时，总计少了它（2405s 实为 22 个 crate 之和）。现在由
#    scripts/run_one_chunk.sh setsid 执行并 O_APPEND 落盘，父 shell 只轮询结果。
#
# 退出码：只要有任何 crate 失败就返回 1，并列出失败清单。
# 用法：
#   bash scripts/run_test_chunks.sh            # 跳过已完成的
#   bash scripts/run_test_chunks.sh --reset    # 清空进度重来
#   CRATE_TIMEOUT=300 bash scripts/run_test_chunks.sh   # 单 crate 超时秒数

set -uo pipefail

PROGRESS=".testall-progress"
LOG=".testall-chunk.log"
# 单个状态的原子记录：每行 "<crate> <status> <秒>"，status ∈ ok|timeout|fail。
# 它同时回答「哪些跑过了」和「总共要多久」，所以 progress 与 timings 没必要
# 分成两个文件——分成两个就会出现「已通过但耗时丢了」或反过来的不一致
# （实测踩过：2405s 少算了 sisyphus-core）。一行同时记两件事，写一次就一致。
# 写入由 scripts/run_one_chunk.sh 用 O_APPEND 单行追加完成，父 shell 被杀也照写。
STATE=".testall-state"
CRATE_TIMEOUT="${CRATE_TIMEOUT:-600}"

# 先切到仓库根，下面的相对路径都以此为基准。
cd "$(dirname "$0")/.." || exit 1

# 单个 crate 的执行器。从 $0 推导而非 $(pwd)：调用者可能在任意目录下
# 执行本脚本，$(pwd) 会指向错误的仓库根。
WRAPPER="$(pwd)/scripts/run_one_chunk.sh"

if [ "${1:-}" = "--reset" ]; then
    rm -f "$PROGRESS" "$LOG" "$STATE" "$STATE".log.*
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

# 从单行状态文件里读某个 crate 的最后一条结果。
# 用 grep 而不是 source/eval：状态文件是磁盘产物，当成数据而非代码执行。
last_result() {
    awk -v p="$1" '$1==p {r=$2; t=$3} END {if (r!="") print r, t}' "$STATE" 2>/dev/null
}

for pkg in "${TARGETS[@]}"; do
    prev="$(last_result "$pkg")"
    if [ -n "$prev" ]; then
        set -- $prev
        if [ "$1" = ok ]; then
            echo "⏭️   $pkg（已通过 ${2}s，跳过）"
            done_count=$((done_count + 1))
            continue
        fi
        # 之前失败/超时的会重跑——它们不写 ok，状态行留着以便追溯。
    fi

    printf '▶️  %-32s ' "$pkg"

    # 交给独立 wrapper 执行并记录。它 setsid 脱离父进程组，所以父 shell
    # 被外部超时杀掉时，它照样跑完并写下结果行。父 shell 只负责等结果。
    before=$(wc -l < "$STATE" 2>/dev/null || echo 0)
    bash "$WRAPPER" "$pkg" "$STATE" "$CRATE_TIMEOUT"
    rc=$?
    after=$(wc -l < "$STATE" 2>/dev/null || echo 0)

    # wrapper 已 setsid 脱离，会立刻返回；这里轮询等结果行落盘。
    # 上限 = CRATE_TIMEOUT + 余量，超了说明 wrapper 没能记录，如实报告而不是
    # 把它当成「通过」——「没记录」和「通过」必须区分开。
    waited=0
    while [ "$after" -le "$before" ] && [ "$waited" -lt $((CRATE_TIMEOUT + 120)) ]; do
        sleep 2
        waited=$((waited + 2))
        after=$(wc -l < "$STATE" 2>/dev/null || echo 0)
    done

    if [ "$after" -le "$before" ]; then
        printf '❓ 未记录结果（等 %ss 仍无记录行）\n' "$waited"
        failed_list+=("$pkg (无结果记录)")
        fail_count=$((fail_count + 1))
        continue
    fi

    set -- $(last_result "$pkg")
    status="${1:-}"; elapsed="${2:-0}"

    if [ "$status" = ok ]; then
        printf '✅ %ss\n' "$elapsed"
        done_count=$((done_count + 1))
    elif [ "$status" = timeout ]; then
        printf '⏱️  %ss 超时（上限 %ss）\n' "$elapsed" "$CRATE_TIMEOUT"
        failed_list+=("$pkg (超时 ${elapsed}s)")
        fail_count=$((fail_count + 1))
    else
        printf '❌ %ss\n' "$elapsed"
        failed_list+=("$pkg (exit $rc)")
        fail_count=$((fail_count + 1))
        grep -E '^test result: FAILED|^error(\[|:)' "$STATE".log."$pkg" 2>/dev/null | head -5 | sed 's/^/      /'
    fi
done

echo ""
echo "━━━ 结果 ━━━"
echo "通过 $done_count / ${#TARGETS[@]}，失败 $fail_count"

# 总耗时从状态文件求和，而不是靠 stdout 累加。
# 只累加 status=ok 的行：失败和超时也会留下行，累加进去会让「总耗时」
# 看起来像一轮完整通过的耗时。
if [ -s "$STATE" ]; then
    total=$(awk '$2=="ok" {s+=$3} END {print s+0}' "$STATE")
    n=$(awk '$2=="ok"' "$STATE" | wc -l)
    echo "已记录耗时: ${total}s（$n 个 crate 通过，来自 $STATE）"
    if [ "$n" -lt "${#TARGETS[@]}" ]; then
        echo "  ℹ️  本次只跑了 $n 个——已通过的会跳过。完整一轮需先 make test-all-chunk-reset"
    fi
    slowest=$(awk '$2=="ok"' "$STATE" | sort -k3 -rn | head -3 | awk '{printf "%s=%ss ", $1, $3}')
    echo "  最慢: $slowest"
fi
if [ $fail_count -gt 0 ]; then
    echo ""
    echo "失败清单："
    for f in "${failed_list[@]}"; do
        echo "  - $f"
    done
    echo ""
    echo "失败 crate 的完整日志保留在 ${STATE}.log.*"
    exit 1
fi
echo "全部通过"
