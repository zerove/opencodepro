#!/usr/bin/env bash
# run_one_chunk.sh 的自检：证明它真能在父进程被 SIGKILL 后仍然记录结果。
#
# 为什么必须自检：这套机制的全部价值在于「父 shell 死了也能记」。
# 如果 setsid 没起作用，脚本平时跑起来一切正常——只有真去杀父进程
# 才能发现它是假的。所以本自检做三件事：
#
#   A. 阳性对照：不杀父进程，fake-cargo 应被调用并记录 ok。
#   B. 真实父进程组 SIGKILL：杀父进程组后，wrapper 仍须记录 ok。
#   C. 变异：强制 wrapper 不 setsid，同一场景必须**不再**记录。
#      C 不过就说明 B 没判别力。
#
# ── 一个曾经让本自检完全失效的坑 ──
# 第一版用 `kill -9 <父进程 pid>` 做「杀父进程」。这根本杀不到 wrapper：
# SIGKILL 只终止那一个进程，子进程只是被 init 收养，**照样跑完并记录**。
# 于是 B 假通过、C 假失败，看起来像 setsid 没用，实际上从没测到隔离。
# 真正对应「外部超时把整组进程一起杀掉」的是 `kill -9 -<pgid>`，
# 也就是按**进程组**杀。本自检因此全程按进程组操作。
#
# 同理，B 里的父进程必须「活着等被杀」：wrapper 一 setsid 就立刻返回，
# 父进程会瞬间退出，等我们 pgrep 时它已经没了——那样 B 又变成没杀任何东西。
# 所以父脚本末尾固定 sleep，等测试来杀。
#
# 用 fake cargo 而不是真 cargo：这里要验的是「记录归属哪个进程」，与编译无关。
#
# 用法：bash scripts/selfcheck_run_one_chunk.sh

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

WRAPPER="$(pwd)/scripts/run_one_chunk.sh"
FAKE_DIR="$(mktemp -d)"
STATE="$FAKE_DIR/state"
# 子脚本要用到这些变量，必须导出：它们是另起的进程，不继承未导出的变量。
export WRAPPER FAKE_DIR
trap 'rm -rf "$FAKE_DIR"' EXIT

pass=0
fail=0
ok()  { echo "  ✅ $1"; pass=$((pass+1)); }
bad() { echo "  ❌ $1"; fail=$((fail+1)); }

# 状态行是 "<crate> <status> <秒>" 三段，不能用 grep -x（整行精确匹配），
# 那会把正常的 "demo-crate ok 2" 判为不匹配。
has_ok() { awk -v p="$1" '$1==p && $2=="ok" && NF>=3 {f=1} END {exit !f}' "$STATE" 2>/dev/null; }

# fake cargo 一启动就写标记文件。测试靠这个标记确认「工作负载已经跑起来了」
# 才去杀进程组——否则会在父进程刚写下 pgid、wrapper 还没起来时就动手，
# 杀了个空组，然后误判成「wrapper 没存活」。fake-cargo 本身也要 sleep 一段，
# 确保杀进程组时它确实在跑。
cat > "$FAKE_DIR/fake-cargo" <<'EOF'
#!/usr/bin/env bash
echo started > "$FAKE_DIR/cargo-started"
sleep "${FAKE_SLEEP:-6}"
exit "${FAKE_EXIT:-0}"
EOF
chmod +x "$FAKE_DIR/fake-cargo"

# 父脚本：跑 wrapper，然后**活着等**被测试杀掉。
# 它把自己写进 pgid 文件，测试据此按进程组杀——对应外部超时的真实行为。
# NO_SET=1 时强制 wrapper 不 setsid，使其留在本进程组内。
cat > "$FAKE_DIR/parent.sh" <<'EOF'
#!/usr/bin/env bash
echo $$ > "$FAKE_DIR/parent.pgid"
if [ -z "${WRAPPER:-}" ] || [ ! -f "$WRAPPER" ]; then
    echo "PARENT-SETUP-ERROR: WRAPPER='${WRAPPER:-}'" >&2
    exit 99
fi
if [ "${NO_SET:-0}" = 1 ]; then
    RUN_ONE_CHUNK_NO_SETSID=1 bash "$WRAPPER" "$1" "$2" 60 "$3"
else
    bash "$WRAPPER" "$1" "$2" 60 "$3"
fi
sleep 60   # 保持存活，等测试杀我们
EOF

# 起一个父进程组，杀掉它，等结果行落盘。
# 参数：<crate名> <是否禁用 setsid:0|1>
run_and_kill_group() {
    local crate="$1" noset="$2" pid=""
    : > "$STATE"
    rm -f "$FAKE_DIR/parent.pgid" "$FAKE_DIR/cargo-started"
    NO_SET="$noset" setsid -f bash "$FAKE_DIR/parent.sh" "$crate" "$STATE" "$FAKE_DIR/fake-cargo"
    # 等父进程写出自己的 pgid
    for _ in $(seq 1 30); do
        [ -s "$FAKE_DIR/parent.pgid" ] && break
        sleep 0.2
    done
    if [ ! -s "$FAKE_DIR/parent.pgid" ]; then
        echo "PARENT-NEVER-STARTED"
        return 2
    fi
    # **关键**：等 fake-cargo 真的跑起来再杀。不等就会在 wrapper 还没
    # 启动时杀掉空进程组，然后误报「wrapper 没能在父进程死后存活」。
    for _ in $(seq 1 60); do
        [ -f "$FAKE_DIR/cargo-started" ] && break
        sleep 0.2
    done
    if [ ! -f "$FAKE_DIR/cargo-started" ]; then
        echo "CARGO-NEVER-STARTED"
        return 2
    fi
    pid=$(cat "$FAKE_DIR/parent.pgid")
    # 按**进程组**杀：负号是关键，只杀单个 pid 杀不到子进程。
    kill -9 -- "-$pid" 2>/dev/null
    # 等 wrapper 跑完（fake-cargo 睡 6s）
    for _ in $(seq 1 40); do
        grep -q "$crate" "$STATE" 2>/dev/null && break
        sleep 0.5
    done
    return 0
}

echo "A. 阳性对照（不杀任何进程）"
: > "$STATE"
FAKE_SLEEP=2 RUN_ONE_CHUNK_NO_SETSID=1 \
    bash "$WRAPPER" demo-crate "$STATE" 60 "$FAKE_DIR/fake-cargo"
rc=$?
if has_ok demo-crate; then
    ok "fake cargo 被调用且记录为 ok"
else
    bad "没记录 ok，状态文件：$(cat "$STATE" 2>/dev/null | tr '\n' '|')"
fi
[ $rc -eq 0 ] && ok "退出码 0" || bad "退出码 $rc，期望 0"

echo "B. 父进程组被 SIGKILL 后仍记录"
out=$(run_and_kill_group demo-kill 0); rc=$?
if [ $rc -eq 2 ]; then
    bad "父进程没起来（$out），B 无效"
else
    ok "已按进程组 SIGKILL 父进程组 $(cat "$FAKE_DIR/parent.pgid" 2>/dev/null)"
    if has_ok demo-kill; then
        ok "父进程组被杀后 wrapper 仍记录 ok"
    else
        bad "父进程组被杀后没有记录（状态文件：$(cat "$STATE" 2>/dev/null | tr '\n' '|')）"
    fi
fi

echo "C. 变异对照（强制禁用 setsid）"
out=$(run_and_kill_group demo-mut 1); rc=$?
if [ $rc -eq 2 ]; then
    bad "变异父进程没起来（$out），C 无效"
else
    ok "已按进程组 SIGKILL 变异父进程组 $(cat "$FAKE_DIR/parent.pgid" 2>/dev/null)"
    # 变异后不该有 ok 记录。若仍有，说明 setsid 根本没起作用，B 的结论不可信。
    if has_ok demo-mut; then
        bad "变异后仍记录 ok——setsid 没起作用，B 的结论不可信"
    else
        ok "变异后不再记录 ok（符合预期，证明 B 在测进程组隔离）"
    fi
fi

echo ""
echo "通过 $pass，失败 $fail"
[ $fail -eq 0 ] || exit 1