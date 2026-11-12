#!/usr/bin/env bash
# L3「dev 权限闸」验收：CLI 的 `sisyphus dev` 必须在执行模型提出的命令前
# 走统一权限体系（与 TUI 的 dev 路径同一口径）。
#
# 背景：CLI 的 dev 路径此前**完全没有 approver** —— 计划里的 `RunCommand`
# （任意 shell）直接执行，既不匹配规则、也不写审计、更不会询问。TUI 路径
# 一直有审批闸，两条路径行为不一致，自动化入口（脚本/CI）尤其危险。
#
# 本脚本用**桩 LLM**（返回固定计划，计划里带一条 RunCommand 步骤）做确定性验收：
#   1) 非交互 + 无 --yes   → 命令被拒绝（fail-safe），命令未执行，审计 asked-denied
#   2) 非交互 + --yes      → 命令放行并执行，审计 asked-approved
#   3) 显式 deny 规则      → 即使 --yes 也拒绝（deny 优先）
#
# 用法：
#   ./scripts/l3-dev-permission-check.sh [--cli PATH]
#
# 退出码：0 = 全部检查通过；1 = 有检查失败。

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="${SISYPHUS_BIN:-$REPO_ROOT/target/debug/sisyphus}"
TMP_ROOT="${TMPDIR:-/tmp}/opencode"
WORKDIR="$TMP_ROOT/l3-dev-perm"
MOCK_PORT="${MOCK_PORT:-11510}"

while [ $# -gt 0 ]; do
  case "$1" in
    --cli) CLI="$2"; shift 2 ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

mkdir -p "$WORKDIR"
PASS=0
FAIL=0

check() { # check <name> <expected-substring> <actual>
  local name="$1" expect="$2" actual="$3"
  if printf '%s' "$actual" | grep -qF -- "$expect"; then
    printf 'PASS  %s\n' "$name"
    PASS=$((PASS + 1))
  else
    printf 'FAIL  %s（期望包含: %s）\n---\n%s\n---\n' "$name" "$expect" "$actual"
    FAIL=$((FAIL + 1))
  fi
}

# ── 准备：固定计划 + 桩 LLM + 配置 ───────────────────────────
MARKER="$WORKDIR/marker.txt"
PLAN="$WORKDIR/plan.json"
cat > "$PLAN" <<JSON
{
  "goal": "权限闸验收",
  "overview": "固定计划：执行一条命令",
  "steps": [
    {
      "id": 1,
      "description": "创建标记文件",
      "action": { "type": "RunCommand", "params": { "command": "touch $MARKER" } },
      "expected_outcome": "标记文件存在",
      "verification": { "type": "None" },
      "status": "Pending"
    }
  ]
}
JSON

CFG="$WORKDIR/config.toml"
cat > "$CFG" <<TOML
bridge_address = "http://127.0.0.1:15401"
http_address = "http://127.0.0.1:15402"

[llm]
api_key = "local"
endpoint = "http://127.0.0.1:$MOCK_PORT/v1/chat/completions"
model = "mock-llm"
request_timeout_secs = 60

[[llm.providers]]
name = "mock"
kind = "openai"
endpoint = "http://127.0.0.1:$MOCK_PORT/v1/chat/completions"
api_key = "local"
model = "mock-llm"
priority = 1
enabled = true
TOML

python3 "$REPO_ROOT/scripts/l3-mock-llm.py" --port "$MOCK_PORT" --content-file "$PLAN" \
  >"$WORKDIR/mock.log" 2>&1 &
MOCK_PID=$!
cleanup() {
  [ -n "${MOCK_PID:-}" ] && kill "$MOCK_PID" 2>/dev/null
  wait "${MOCK_PID:-}" 2>/dev/null
}
trap cleanup EXIT
for _ in $(seq 1 40); do
  (exec 3<>"/dev/tcp/127.0.0.1/$MOCK_PORT") 2>/dev/null && break
  sleep 0.25
done

if [ ! -x "$CLI" ]; then
  echo "FAIL  CLI 不可用（$CLI），先 cargo build -p sisyphus-cli" >&2
  exit 1
fi

# 审计路径由 `--data-dir` 决定（main.rs 固定写到 <data-dir>/audit/permissions.jsonl）
DATA_DIR="$WORKDIR/data"
AUDIT_PATH="$DATA_DIR/audit/permissions.jsonl"

run_dev() { # run_dev <extra-args...>；stdout+stderr 合并
  rm -f "$MARKER" "$AUDIT_PATH"
  ( cd "$WORKDIR" && timeout 120 "$CLI" --config "$CFG" --data-dir "$DATA_DIR" \
      dev "创建标记文件" --timeout-secs 60 "$@" ) >"$WORKDIR/dev.out" 2>&1
  echo $? >"$WORKDIR/dev.rc"
}

# ── 1) 非交互 + 无 --yes → 必须拒绝 ─────────────────────────
rm -f "$WORKDIR/.sisyphus-permissions.json"
run_dev
OUT1="$(cat "$WORKDIR/dev.out")"
if [ -f "$MARKER" ]; then
  printf 'FAIL  非交互无 --yes 时命令不应执行（标记文件已生成）\n'
  FAIL=$((FAIL + 1))
else
  printf 'PASS  非交互无 --yes 时命令未执行\n'
  PASS=$((PASS + 1))
fi
check "非交互拒绝时输出说明" "非交互环境无法确认" "$OUT1"
check "非交互拒绝时审计记录 asked-denied" "asked-denied" "$(cat "$AUDIT_PATH" 2>/dev/null)"

# ── 2) 非交互 + --yes → 必须放行并执行 ──────────────────────
run_dev --yes
if [ -f "$MARKER" ]; then
  printf 'PASS  --yes 时命令已执行\n'
  PASS=$((PASS + 1))
else
  printf 'FAIL  --yes 时命令应执行（标记文件缺失）\n---\n%s\n---\n' "$(cat "$WORKDIR/dev.out")"
  FAIL=$((FAIL + 1))
fi
check "--yes 放行时审计记录 asked-approved" "asked-approved" "$(cat "$AUDIT_PATH" 2>/dev/null)"

# ── 3) 显式 deny 规则 → 即使 --yes 也拒绝 ───────────────────
# 显式 deny 规则（默认读当前目录的 .sisyphus-permissions.json）
cat > "$WORKDIR/.sisyphus-permissions.json" <<'JSON'
[
  { "pattern": "builtin:bash_exec", "permission": "deny" }
]
JSON
rm -f "$AUDIT_PATH" "$MARKER"
(
  cd "$WORKDIR" && timeout 120 "$CLI" --config "$CFG" --data-dir "$DATA_DIR" \
    dev "创建标记文件" --timeout-secs 60 --yes
) >"$WORKDIR/dev3.out" 2>&1
if [ -f "$MARKER" ]; then
  printf 'FAIL  deny 规则下命令不应执行（标记文件已生成）\n'
  FAIL=$((FAIL + 1))
else
  printf 'PASS  deny 规则下命令未执行\n'
  PASS=$((PASS + 1))
fi
check "deny 规则优先于 --yes（输出说明）" "已拒绝" "$(cat "$WORKDIR/dev3.out")"
check "deny 规则优先于 --yes（审计 denied）" '"resolved":"denied"' "$(cat "$AUDIT_PATH" 2>/dev/null)"

# ── 4) exec 入口：stdin EOF 必须 fail-safe（不得当成「回车同意」）────────
rm -f "$MARKER"
( cd "$WORKDIR" && timeout 60 "$CLI" --config "$CFG" --data-dir "$DATA_DIR" \
    exec "touch $MARKER" < /dev/null ) >"$WORKDIR/exec-eof.out" 2>&1
if [ -f "$MARKER" ]; then
  printf 'FAIL  exec 在 stdin EOF 时不应执行（fail-open）\n'
  FAIL=$((FAIL + 1))
else
  printf 'PASS  exec 在 stdin EOF 时未执行\n'
  PASS=$((PASS + 1))
fi
check "exec 在 stdin EOF 时给出明确错误" "stdin reached EOF" "$(cat "$WORKDIR/exec-eof.out")"

# ── 5) exec 入口：显式 y 仍然放行（向后兼容）────────────────────────
rm -f "$MARKER"
( cd "$WORKDIR" && echo y | timeout 60 "$CLI" --config "$CFG" --data-dir "$DATA_DIR" \
    exec "touch $MARKER" ) >"$WORKDIR/exec-yes.out" 2>&1
if [ -f "$MARKER" ]; then
  printf 'PASS  exec 显式 y 仍放行\n'
  PASS=$((PASS + 1))
else
  printf 'FAIL  exec 显式 y 应放行\n---\n%s\n---\n' "$(cat "$WORKDIR/exec-yes.out")"
  FAIL=$((FAIL + 1))
fi

# ── 6) delegate inline：执行命令前也要过权限闸 ─────────────────────
rm -f "$MARKER"
( cd "$WORKDIR" && timeout 60 "$CLI" --config "$CFG" --data-dir "$DATA_DIR" \
    delegate inline "touch $MARKER" < /dev/null ) >"$WORKDIR/delegate.out" 2>&1
if [ -f "$MARKER" ]; then
  printf 'FAIL  delegate inline 在未获授权时不应执行\n'
  FAIL=$((FAIL + 1))
else
  printf 'PASS  delegate inline 在未获授权时未执行\n'
  PASS=$((PASS + 1))
fi
check "delegate inline 被权限策略拒绝" "权限策略拒绝" "$(cat "$WORKDIR/delegate.out")"

# ── 7) delegate run（任务文件）：每条命令都要过权限闸 ─────────────────
cat > "$WORKDIR/tasks.json" <<JSON
[
  { "id": "t1", "name": "touch marker", "command": "touch $MARKER" }
]
JSON
rm -f "$MARKER"
( cd "$WORKDIR" && timeout 60 "$CLI" --config "$CFG" --data-dir "$DATA_DIR" \
    delegate run tasks.json < /dev/null ) >"$WORKDIR/delegate-run.out" 2>&1
if [ -f "$MARKER" ]; then
  printf 'FAIL  delegate run 在未获授权时不应执行\n'
  FAIL=$((FAIL + 1))
else
  printf 'PASS  delegate run 在未获授权时未执行\n'
  PASS=$((PASS + 1))
fi

echo
echo "PASS=$PASS FAIL=$FAIL"
[ "$FAIL" -eq 0 ]
