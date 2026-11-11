#!/usr/bin/env bash
# T3.3：桥接启动分段计时测量（阶段初始化耗时）。
# 用法: ./scripts/measure-bridge-startup.sh
# 输出: startup_phase_*_ms（config / otel / runtime_init / bridge / http_listening / grpc_listening / ready）
#       ready_ms = 进程启动 → /health 就绪的端到端耗时（与 single-node-bench-sample.sh bridge 同口径）
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/measure-bridge-startup.sh

Measures bridge startup phases (config load / runtime init / bridge init /
HTTP listener / gRPC listener) and end-to-end readiness (start → /health OK).

Env:
  N=1 等待 /health 的轮询超时（秒）
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

BIN="${1:-./target/debug/sisyphus-bridge}"
N="${N:-60}"
CFG="${CFG:-/tmp/opencode/sn-bench-bridge.toml}"
HTTP="${HTTP:-http://127.0.0.1:15291}"
LOG="$(mktemp /tmp/opencode/bridge-startup-log.XXXXXX)"
READY_MS_FILE="$(mktemp /tmp/opencode/bridge-ready.XXXXXX)"

cleanup() {
  if [ -n "${BPID:-}" ]; then
    kill "$BPID" 2>/dev/null || true
    wait "$BPID" 2>/dev/null || true
  fi
  rm -f "$LOG" "$READY_MS_FILE"
}
trap cleanup EXIT

cat > "$CFG" <<TOML
bridge_address = "http://127.0.0.1:15261"
http_address = "$HTTP"

[cluster]
enabled = false
TOML

: > "$READY_MS_FILE"
SISYPHUS_CONFIG="$CFG" SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true "$BIN" > "$LOG" 2>&1 &
BPID=$!
START_MS=$(date +%s%3N)

ready=""
for i in $(seq 1 "$N"); do
  if curl -sf --max-time 1 "$HTTP/health" >/dev/null 2>&1; then
    READY_MS=$(date +%s%3N)
    printf '%s\n' "$((READY_MS - START_MS))" > "$READY_MS_FILE"
    ready="1"
    break
  fi
  sleep 0.5
done

if [ -z "$ready" ]; then
  echo "Bridge did not become ready within ${N}s" >&2
  exit 2
fi

kill "$BPID" 2>/dev/null || true
wait "$BPID" 2>/dev/null || true
BPID=""

python3 - "$LOG" "$READY_MS_FILE" <<'PY'
import json, re, sys

log_path, ready_path = sys.argv[1], sys.argv[2]
ready_ms = int(open(ready_path).read().strip())
phases = {}
for line in open(log_path, encoding="utf-8", errors="replace"):
    m = re.search(r"startup_phase ([a-z_]+)_ms=(\d+)", line)
    if m:
        phases[m.group(1)] = int(m.group(2))
phases["ready_ms"] = ready_ms

result = {
    "phases_ms": phases,
    "end_to_end_ready_ms": ready_ms,
}
print(json.dumps(result, indent=2))
PY

rc=$?
exit $rc
