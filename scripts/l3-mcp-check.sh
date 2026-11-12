#!/usr/bin/env bash
# L3「MCP 场景」验收：用一个**独立实现**的 MCP 服务器验证客户端互通性。
#
# 背景：此前 MCP 只有自家 mock（round-trip）测试，线上协议字段与「通知等待响应」
# 两处实现都偏离规范，导致连不上任何合规 MCP 服务器。本脚本用真实协议服务器
# 做端到端验收：
#
#   级别 1（CLI）：sisyphus mcp info / mcp call / resources / prompts ——
#                 校验握手、工具列表与调用、资源列表与读取、提示列表与获取
#                 （stdio 与 HTTP 两种传输各跑一遍）
#   级别 2（引擎）：启动 bridge（同时挂 stdio 与 HTTP 两个 MCP 服务器），校验
#                   /api/tools 出现 MCP 工具，并经 /api/tools/execute 真正调用一次
#
# 用法：
#   ./scripts/l3-mcp-check.sh [--cli-only] [--bridge-bin PATH]
#
# 退出码：0 = 全部检查通过；1 = 有检查失败（逐项输出 PASS/FAIL）。

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="${SISYPHUS_BIN:-$REPO_ROOT/target/debug/sisyphus}"
BRIDGE_BIN="${BRIDGE_BIN:-$REPO_ROOT/target/debug/sisyphus-bridge}"
SERVER="$REPO_ROOT/scripts/l3-mcp-server.py"
HTTP_SERVER="$REPO_ROOT/scripts/l3-mcp-http-server.py"
HTTP_MCP_PORT="${HTTP_MCP_PORT:-11520}"
CLI_ONLY=0
TMP_ROOT="${TMPDIR:-/tmp}/opencode"
WORKDIR="$TMP_ROOT/l3-mcp"
HTTP_PORT="${HTTP_PORT:-15391}"
GRPC_PORT="${GRPC_PORT:-15361}"
BASE="http://127.0.0.1:$HTTP_PORT"

while [ $# -gt 0 ]; do
  case "$1" in
    --cli-only) CLI_ONLY=1; shift ;;
    --bridge-bin) BRIDGE_BIN="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
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

echo "cli:     $CLI"
echo "server:  $SERVER"
echo

# ── 级别 1：CLI 走 MCP 协议 ──────────────────────────────────
if [ ! -x "$CLI" ]; then
  echo "FAIL  CLI 不可用（$CLI），先 cargo build -p sisyphus-cli" >&2
  exit 1
fi

info_out="$(timeout 60 "$CLI" mcp info python3 "$SERVER" 2>&1)"
check "mcp info 握手并列出 3 个工具" "Available tools (3)" "$info_out"
check "mcp info 工具名 sum" "sum" "$info_out"
check "mcp info 工具名 word_count" "word_count" "$info_out"

sum_out="$(timeout 60 "$CLI" mcp call sum '{"a":2,"b":3}' python3 "$SERVER" 2>&1)"
check "mcp call sum(2,3) = 5" '"text": "5"' "$sum_out"

wc_out="$(timeout 60 "$CLI" mcp call word_count '{"text":"a b c"}' python3 "$SERVER" 2>&1)"
check "mcp call word_count = 3" '"text": "3"' "$wc_out"

err_out="$(timeout 60 "$CLI" mcp call sum '{}' python3 "$SERVER" 2>&1)"
check "mcp call 参数缺失返回 isError" '"isError": true' "$err_out"

res_out="$(timeout 60 "$CLI" mcp resources python3 "$SERVER" 2>&1)"
check "mcp resources 列出 2 个资源" '"memo://welcome"' "$res_out"
check "mcp resources 包含 status" '"memo://status"' "$res_out"
read_out="$(timeout 60 "$CLI" mcp read-resource 'memo://welcome' python3 "$SERVER" 2>&1)"
check "mcp read-resource 返回文本内容" 'hello from mcp resource' "$read_out"
prompts_out="$(timeout 60 "$CLI" mcp prompts python3 "$SERVER" 2>&1)"
check "mcp prompts 列出 greet" '"greet"' "$prompts_out"
check "mcp prompts 列出 summary" '"summary"' "$prompts_out"
prompt_out="$(timeout 60 "$CLI" mcp get-prompt greet '{"name":"Sisyphus"}' python3 "$SERVER" 2>&1)"
check "mcp get-prompt 返回渲染消息" 'Say hello to Sisyphus' "$prompt_out"

# 客户端 ↔ 自家 MCP server（`sisyphus mcp serve`）：同一套协议的两端。
# 这里同时守住「自家 server 不得回复 notification」——否则滞留在管道里的响应
# 会被下一个请求读走，客户端报「list_tools: missing result」。
self_out="$(timeout 60 "$CLI" mcp info "$CLI" mcp serve 2>&1)"
check "自家 MCP server 可被协议客户端读取工具列表" "Available tools (" "$self_out"
check "自家 MCP server 工具名 metis_analyse" "metis_analyse" "$self_out"

# ── 级别 1b：HTTP 传输（Streamable HTTP）────────────────────────
if (exec 3<>"/dev/tcp/127.0.0.1/$HTTP_MCP_PORT") 2>/dev/null; then
  echo "FAIL  HTTP MCP 端口 $HTTP_MCP_PORT 已被占用（可能残留旧服务）" >&2
  exit 1
fi
python3 "$HTTP_SERVER" --port "$HTTP_MCP_PORT" >"$WORKDIR/http-server.log" 2>&1 &
HTTP_PID=$!
READY_HTTP=0
for _ in $(seq 1 40); do
  if (exec 3<>"/dev/tcp/127.0.0.1/$HTTP_MCP_PORT") 2>/dev/null; then
    READY_HTTP=1
    break
  fi
  sleep 0.25
done
if [ "$READY_HTTP" -ne 1 ]; then
  echo "FAIL  HTTP MCP fixture 未就绪（port=$HTTP_MCP_PORT）" >&2
  tail -20 "$WORKDIR/http-server.log" 2>/dev/null || true
  exit 1
fi
HTTP_URL="http://127.0.0.1:$HTTP_MCP_PORT/mcp"

http_info="$(timeout 60 "$CLI" mcp info "$HTTP_URL" 2>&1)"
check "HTTP 传输握手并列出 3 个工具" "Available tools (3)" "$http_info"
http_sum="$(timeout 60 "$CLI" mcp call sum '{"a":20,"b":22}' "$HTTP_URL" 2>&1)"
check "HTTP 传输 mcp call sum(20,22) = 42" '"text": "42"' "$http_sum"
http_err="$(timeout 60 "$CLI" mcp call sum '{}' "$HTTP_URL" 2>&1)"
check "HTTP 传输参数缺失返回 isError" '"isError": true' "$http_err"
http_res="$(timeout 60 "$CLI" mcp resources "$HTTP_URL" 2>&1)"
check "HTTP 传输列出资源 memo://welcome" '"memo://welcome"' "$http_res"
http_read="$(timeout 60 "$CLI" mcp read-resource 'memo://welcome' "$HTTP_URL" 2>&1)"
check "HTTP 传输读取资源内容" 'hello from mcp resource' "$http_read"
http_prompts="$(timeout 60 "$CLI" mcp prompts "$HTTP_URL" 2>&1)"
check "HTTP 传输列出 prompts" '"greet"' "$http_prompts"
http_prompt="$(timeout 60 "$CLI" mcp get-prompt greet '{"name":"Sisyphus"}' "$HTTP_URL" 2>&1)"
check "HTTP 传输 get-prompt 渲染" 'Say hello to Sisyphus' "$http_prompt"

# ── 级别 2：引擎（bridge）侧注册与调用 ───────────────────────
if [ "$CLI_ONLY" -eq 1 ]; then
  echo
  echo "PASS=$PASS FAIL=$FAIL（--cli-only，跳过引擎级检查）"
  [ "$FAIL" -eq 0 ]
  exit $?
fi

if [ ! -x "$BRIDGE_BIN" ]; then
  echo
  echo "SKIP  引擎级检查：bridge 未构建（cargo build -p sisyphus-bridge）"
  echo "PASS=$PASS FAIL=$FAIL"
  [ "$FAIL" -eq 0 ]
  exit $?
fi

CFG="$WORKDIR/config.toml"
cat > "$CFG" <<TOML
bridge_address = "http://127.0.0.1:$GRPC_PORT"
http_address = "http://127.0.0.1:$HTTP_PORT"

[llm]
api_key = "local"
endpoint = "http://127.0.0.1:11434/v1/chat/completions"
model = "qwen2.5-coder:3b"
request_timeout_secs = 600

# 本场景不涉及嵌入；bridge 启动要求显式允许 dummy 嵌入（测试/开发用途）。
[llm.embedding]
allow_dummy_embeddings = true

[mcp]
  [mcp.stdio_servers]
  "l3" = "python3 $SERVER"
  [mcp.http_servers]
  "l3http" = "$HTTP_URL"
TOML

BRIDGE_LOG="$WORKDIR/bridge.log"
# bridge 从 SISYPHUS_CONFIG 读配置（没有 --config 选项）
SISYPHUS_CONFIG="$CFG" "$BRIDGE_BIN" >"$BRIDGE_LOG" 2>&1 &
BPID=$!
cleanup() {
  [ -n "${BPID:-}" ] && kill "$BPID" 2>/dev/null
  wait "${BPID:-}" 2>/dev/null
  [ -n "${HTTP_PID:-}" ] && kill "$HTTP_PID" 2>/dev/null
  wait "${HTTP_PID:-}" 2>/dev/null
}
trap cleanup EXIT

READY=0
for _ in $(seq 1 60); do
  if curl -sf --max-time 2 "$BASE/health" >/dev/null 2>&1; then READY=1; break; fi
  sleep 0.5
done
if [ "$READY" -ne 1 ]; then
  printf 'FAIL  bridge 未就绪（%s）\n' "$BASE/health"
  tail -20 "$BRIDGE_LOG"
  FAIL=$((FAIL + 1))
else
  tools_out="$(curl -sf --max-time 10 "$BASE/api/tools" 2>&1)"
  check "引擎 /api/tools 含 MCP 工具 l3:sum" '"l3:sum"' "$tools_out"
  check "引擎 /api/tools 标注来源为 mcp" '"source":"mcp"' "$tools_out"

  exec_out="$(curl -sf --max-time 20 -X POST "$BASE/api/tools/execute" \
    -H 'content-type: application/json' \
    -d '{"tool_id":"l3:sum","args_json":"{\"a\":40,\"b\":2}"}' 2>&1)"
  check "引擎调用 MCP 工具 l3:sum(40,2)" '42' "$exec_out"
  check "引擎调用返回 SUCCESS" '"status":"SUCCESS"' "$exec_out"

  check "引擎 /api/tools 含 HTTP 传输的 MCP 工具 l3http:sum" '"l3http:sum"' "$tools_out"
  http_exec_out="$(curl -sf --max-time 20 -X POST "$BASE/api/tools/execute" \
    -H 'content-type: application/json' \
    -d '{"tool_id":"l3http:sum","args_json":"{\"a\":1,\"b\":2}"}' 2>&1)"
  check "引擎调用 HTTP 传输的 MCP 工具 l3http:sum(1,2)" '3' "$http_exec_out"
fi

echo
echo "PASS=$PASS FAIL=$FAIL"
[ "$FAIL" -eq 0 ]
