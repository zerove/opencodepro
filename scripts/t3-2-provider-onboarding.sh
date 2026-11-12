#!/usr/bin/env bash
# T3.2 Provider 交互式引导 —— 可复跑验收脚本
#
# 覆盖：
#   A. 成功路径：对一个**真实** OpenAI 兼容端口做 `provider login`（真实 HTTP 调用，
#      测试通过才写配置）→ `provider list` → `provider test` 真实返回 completion
#   B. 失败路径（本地 mock 服务）：401 / 404 / 返回 HTML（协议选错）/ 端点不可达
#      —— 必须失败、给出可执行 hint、且**不写入配置**
#   C. `provider remove` 清理
#
# 用法：
#   ./scripts/t3-2-provider-onboarding.sh [--cli ./target/debug/sisyphus]
#
# 环境变量：
#   LLM_ENDPOINT  真实 provider 的 OpenAI 兼容端点（默认 ollama 本地端点）
#   LLM_MODEL     模型名（默认 qwen2.5-coder:3b）
#   LLM_API_KEY   API key（默认 ollama，本地服务不校验）
#
# 退出码：0 = 全部断言通过；1 = 有断言失败。

set -uo pipefail

CLI="./target/debug/sisyphus"
while [ $# -gt 0 ]; do
  case "$1" in
    --cli) CLI="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

LLM_ENDPOINT="${LLM_ENDPOINT:-http://127.0.0.1:11434/v1/chat/completions}"
LLM_MODEL="${LLM_MODEL:-qwen2.5-coder:3b}"
LLM_API_KEY="${LLM_API_KEY:-ollama}"

TMP_ROOT="${TMPDIR:-/tmp}/opencode"
mkdir -p "$TMP_ROOT"
WORK="$(mktemp -d "$TMP_ROOT/t3-2-XXXXXX")"
CFG="$WORK/config.toml"
MOCK_PORT="${MOCK_PORT:-15987}"
PASS=0
FAIL=0
SKIP=0
RESULTS=()

log() { printf '%s\n' "$*"; }
ok() {
  PASS=$((PASS + 1))
  RESULTS+=("PASS|$1")
  log "PASS  $1"
}
bad() {
  FAIL=$((FAIL + 1))
  RESULTS+=("FAIL|$1")
  log "FAIL  $1"
}
skip() {
  SKIP=$((SKIP + 1))
  RESULTS+=("SKIP|$1")
  log "SKIP  $1"
}

cleanup() {
  [ -n "${MOCK_PID:-}" ] && kill "$MOCK_PID" 2>/dev/null
  wait "${MOCK_PID:-}" 2>/dev/null
}
trap cleanup EXIT

cat > "$CFG" <<TOML
bridge_address = "http://127.0.0.1:15401"
http_address = "http://127.0.0.1:15402"

[llm]
api_key = ""
endpoint = "http://localhost:8000/v1/chat/completions"
model = "gpt-4o"
request_timeout_secs = 180
TOML

log "== 环境 =="
log "cli:      $CLI"
log "endpoint: $LLM_ENDPOINT"
log "model:    $LLM_MODEL"
log "workdir:  $WORK"
log

# ── A. 成功路径 ────────────────────────────────────────────────
log "== A. 成功路径（真实调用） =="
if ! curl -sf --max-time 3 -o /dev/null "${LLM_ENDPOINT%/chat/completions}/models" 2>/dev/null \
   && ! curl -sf --max-time 3 -o /dev/null -X POST -H 'content-type: application/json' \
        -d "{\"model\":\"$LLM_MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":1}" \
        "$LLM_ENDPOINT" 2>/dev/null; then
  skip "A0 真实 provider 不可达（$LLM_ENDPOINT）—— 设 LLM_ENDPOINT 指向可用服务后重跑"
else
  ok "A0 真实 provider 可达"

  OUT="$WORK/a1-login.txt"
  "$CLI" --config "$CFG" provider login \
    --name e2e-local --kind openai \
    --endpoint "$LLM_ENDPOINT" --model "$LLM_MODEL" \
    --api-key "$LLM_API_KEY" --yes --timeout-secs 180 >"$OUT" 2>&1
  rc=$?
  if [ $rc -eq 0 ] && grep -q "✅ PASS" "$OUT" && grep -q "Registered provider 'e2e-local'" "$OUT"; then
    ok "A1 provider login 通过真实连通性测试并写配置"
  else
    bad "A1 provider login 失败（rc=$rc）"
  fi

  if grep -q 'name = "e2e-local"' "$CFG" && grep -q "\\[\\[llm.providers\\]\\]" "$CFG"; then
    ok "A2 配置写入 [[llm.providers]] 且名称为 e2e-local"
  else
    bad "A2 配置未写入"
  fi

  if stat -c '%a' "$CFG" | grep -q '^600$'; then
    ok "A3 含明文密钥的配置权限为 0600"
  else
    bad "A3 配置权限不是 0600（$(stat -c '%a' "$CFG")）"
  fi

  OUT="$WORK/a4-list.txt"
  "$CLI" --config "$CFG" provider list >"$OUT" 2>&1
  if grep -q "e2e-local" "$OUT" && grep -q "key: set" "$OUT"; then
    ok "A4 provider list 显示已注册 provider"
  else
    bad "A4 provider list 未显示 provider"
  fi

  OUT="$WORK/a5-test.txt"
  "$CLI" --config "$CFG" provider test e2e-local --timeout-secs 180 >"$OUT" 2>&1
  rc=$?
  if [ $rc -eq 0 ] && grep -q "✅ PASS" "$OUT" && grep -q "response:" "$OUT"; then
    ok "A5 provider test 真实返回 completion"
  else
    bad "A5 provider test 失败（rc=$rc）"
  fi
fi
log

# ── B. 失败路径（mock） ────────────────────────────────────────
log "== B. 失败路径（mock 服务 :$MOCK_PORT） =="
python3 "$(dirname "$0")/t3-2-mock-llm.py" "$MOCK_PORT" >"$WORK/mock.log" 2>&1 &
MOCK_PID=$!
for _ in $(seq 1 40); do
  curl -sf --max-time 1 -o /dev/null -X POST -d '{}' \
    "http://127.0.0.1:$MOCK_PORT/ok/v1/chat/completions" 2>/dev/null && break
  sleep 0.25
done

expect_fail() {
  local label="$1" endpoint="$2" want_hint="$3" want_status="$4"
  local base="$WORK/b-$want_status.txt"
  cp "$CFG" "$WORK/case-$want_status.toml"
  "$CLI" --config "$WORK/case-$want_status.toml" provider login \
    --name "bad-$want_status" --kind openai --endpoint "$endpoint" \
    --model mock-model --api-key mock-key --yes --timeout-secs 30 >"$base" 2>&1
  local rc=$?
  if [ $rc -eq 0 ]; then
    bad "$label 应失败但返回 0"
    return
  fi
  if ! grep -q "$want_status" "$base"; then
    bad "$label 错误里缺少状态码 $want_status"
    return
  fi
  if ! grep -q "hint:" "$base"; then
    bad "$label 未给出可执行 hint"
    return
  fi
  if ! grep -qF "$want_hint" "$base"; then
    bad "$label hint 未命中期望片段：$want_hint"
    return
  fi
  if grep -q "bad-$want_status" "$WORK/case-$want_status.toml"; then
    bad "$label 失败后仍写入了配置"
    return
  fi
  ok "$label 失败 + 状态码 + 可执行 hint + 未写配置"
}

expect_fail "B1 401 密钥无效" "http://127.0.0.1:$MOCK_PORT/401/v1/chat/completions" "provider login" "401"
expect_fail "B2 404 端点/模型不存在" "http://127.0.0.1:$MOCK_PORT/404/v1/chat/completions" "v1/chat/completions" "404"

# B3 协议选错：返回 HTML
cp "$CFG" "$WORK/case-html.toml"
OUT="$WORK/b-html.txt"
"$CLI" --config "$WORK/case-html.toml" provider login \
  --name bad-html --kind openai \
  --endpoint "http://127.0.0.1:$MOCK_PORT/html/v1/chat/completions" \
  --model mock-model --api-key mock-key --yes --timeout-secs 30 >"$OUT" 2>&1
rc=$?
if [ $rc -ne 0 ] && grep -q "OpenAI 兼容 JSON" "$OUT" && ! grep -q "bad-html" "$WORK/case-html.toml"; then
  ok "B3 协议选错（HTML）失败 + hint 指向 kind + 未写配置"
else
  bad "B3 协议选错未按预期失败（rc=$rc）"
fi

# B4 端点不可达
cp "$CFG" "$WORK/case-dead.toml"
OUT="$WORK/b-dead.txt"
"$CLI" --config "$WORK/case-dead.toml" provider login \
  --name bad-dead --kind openai \
  --endpoint "http://127.0.0.1:1/v1/chat/completions" \
  --model mock-model --api-key mock-key --yes --timeout-secs 20 >"$OUT" 2>&1
rc=$?
if [ $rc -ne 0 ] && grep -q "hint:" "$OUT" && ! grep -q "bad-dead" "$WORK/case-dead.toml"; then
  ok "B4 端点不可达失败 + hint + 未写配置"
else
  bad "B4 端点不可达未按预期失败（rc=$rc）"
fi
log

# ── C. remove ────────────────────────────────────────────────
log "== C. provider remove =="
OUT="$WORK/c1-remove.txt"
"$CLI" --config "$CFG" provider remove e2e-local >"$OUT" 2>&1
rc=$?
if [ $rc -eq 0 ] && ! grep -q 'name = "e2e-local"' "$CFG"; then
  ok "C1 provider remove 移除条目"
else
  bad "C1 provider remove 失败（rc=$rc）"
fi

OUT="$WORK/c2-remove-missing.txt"
"$CLI" --config "$CFG" provider remove nope >"$OUT" 2>&1
rc=$?
if [ $rc -ne 0 ] && grep -q "未找到 provider" "$OUT"; then
  ok "C2 remove 不存在的 provider 报错可读"
else
  bad "C2 remove 不存在的 provider 未按预期报错（rc=$rc）"
fi
log

# ── 汇总 ─────────────────────────────────────────────────────
log "== 汇总 =="
log "PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
log "workdir: $WORK"
{
  echo "# T3.2 provider onboarding acceptance"
  echo
  echo "run_at: $(date -Iseconds)"
  echo "cli: $CLI"
  echo "endpoint: $LLM_ENDPOINT"
  echo "model: $LLM_MODEL"
  echo
  echo "| 结果 | 检查项 |"
  echo "|---|---|"
  for r in "${RESULTS[@]}"; do
    echo "| ${r%%|*} | ${r#*|} |"
  done
  echo
  echo "PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
} > "$WORK/summary.md"

[ "$FAIL" -eq 0 ]
