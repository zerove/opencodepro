#!/usr/bin/env bash
# 跑**单个** crate 的测试，并把结果落盘。
#
# 存在的理由：run_test_chunks.sh 的父 shell 被外部超时杀掉时，
# 正在跑的 crate 会成为孤儿进程继续跑完并通过，但**没有任何人记录它的耗时**。
# 实测踩过：2405s 少算了 sisyphus-core（它作为孤儿跑完，✅ 行随 stdout 丢了）。
# 把「执行 + 记录」整体挪进这个独立 wrapper，并用 setsid 把它甩出父进程的
# 进程组，父 shell 被杀也照样记录。
#
# 它自己绝不能被超时杀掉父进程以外的信号打断，所以不用 `set -e`：
# 记录失败必须以非零退出码暴露给父 shell，而不是静默消失。
#
# 用法：
#   bash scripts/run_one_chunk.sh <pkg> <state_file> <timeout_s> [cargo_bin]

set -uo pipefail

pkg="${1:?用法: run_one_chunk.sh <pkg> <state_file> <timeout_s> [cargo_bin]}"
state_file="${2:?缺少 state_file}"
crate_timeout="${3:?缺少 timeout_s}"
# CARGO_BIN 是给自检用的接缝：真实调用走 cargo，自检塞一个假的。
# 默认值是 cargo，所以生产路径完全不受影响。
cargo_bin="${4:-cargo}"

# setsid：把自己放进新会话，脱离父进程的进程组。父 shell 被 SIGKILL
# 时不会连带收到信号。父 shell 不在了也要继续跑到记录完成。
# 已经不在会话首进程时才需要 -f（例如用 setsid 从别处再包一层）。
if command -v setsid >/dev/null 2>&1; then
    if [ "$#" -gt 0 ] && [ "${RUN_ONE_CHUNK_NO_SETSID:-0}" != "1" ]; then
        RUN_ONE_CHUNK_NO_SETSID=1 setsid -f bash "$0" "$pkg" "$state_file" "$crate_timeout" "$cargo_bin" \
            > /dev/null 2>&1
        exit 0
    fi
fi

start=$(date +%s)

# 状态文件的写权限在这里就要探明：孤儿进程写不进去的话，等于白跑。
if ! ( : >> "$state_file" ) 2>/dev/null; then
    echo "run_one_chunk: 无法写 $state_file" >&2
    exit 70
fi

timeout "$crate_timeout" \
    env CARGO_BUILD_JOBS=1 "$cargo_bin" test -p "$pkg" --no-fail-fast \
    > "$state_file.log.$pkg" 2>&1
rc=$?
elapsed=$(( $(date +%s) - start ))

case $rc in
    0) status=ok ;;
    124) status=timeout ;;
    *) status=fail ;;
esac

# 单行追加 + O_APPEND：小于 PIPE_BUF 的写入在 POSIX 下对并发写者是原子的，
# 因此父 shell 和孤儿进程同时追加也不会互相截断。合并 progress 与 timings
# 成这一个文件后，「已通过」和「耗时多少」就再也不会不一致了。
printf '%s %s %s\n' "$pkg" "$status" "$elapsed" >> "$state_file"

if [ "$status" = ok ]; then
    rm -f "$state_file.log.$pkg"
fi
exit $rc