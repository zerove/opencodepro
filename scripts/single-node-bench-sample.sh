#!/usr/bin/env bash
# P2-B: 单机 CLI/bridge 轻量采样脚本（多次取样，输出摘要）
# 用法:
#   ./scripts/single-node-bench-sample.sh cli
#   ./scripts/single-node-bench-sample.sh bridge
#   ./scripts/single-node-bench-sample.sh --help
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/single-node-bench-sample.sh <cli|bridge>

Modes:
  cli     对 debug/release CLI 常用命令做 N 次采样并输出 min/avg/max
  bridge  对 bridge 启动就绪与只读端点延迟做 N 次采样并输出 min/avg/max

Env:
  N=5
  CLI_BIN=./target/debug/sisyphus
  BRIDGE_BIN=./target/debug/sisyphus-bridge
  CFG=/tmp/opencode/sn-bench-bridge.toml
  HTTP=http://127.0.0.1:15291
  GRPC=http://127.0.0.1:15261
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

MODE="${1:-cli}"
N="${N:-5}"
CLI_BIN="${CLI_BIN:-./target/debug/sisyphus}"
BRIDGE_BIN="${BRIDGE_BIN:-./target/debug/sisyphus-bridge}"

stats() {
  # read numbers from stdin, one per line
  awk '
    NF { v=$1+0; n++; sum+=v; if(n==1||v<min)min=v; if(n==1||v>max)max=v; }
    END {
      if(n==0){ print "n=0"; exit }
      printf "n=%d min=%.6f avg=%.6f max=%.6f\n", n, min, sum/n, max
    }'
}

case "$MODE" in
  cli)
    echo "== CLI bench sample =="
    echo "CLI_BIN=$CLI_BIN N=$N"
    if [ ! -x "$CLI_BIN" ]; then
      echo "[WARN] CLI_BIN not executable: $CLI_BIN" >&2
    fi
    mkdir -p /tmp/opencode/bench-sn
    for cmd in "--help" "exec echo ok" "--data-dir /tmp/opencode/bench-sn session list"; do
      echo "-- sisyphus $cmd --"
      for i in $(seq 1 "$N"); do
        # shellcheck disable=SC2086
        t=$(/usr/bin/time -p $CLI_BIN $cmd >/dev/null 2> /tmp/sn-time.err; awk '/^real/{print $2}' /tmp/sn-time.err)
        echo "$t"
      done | stats
    done
    ;;
  bridge)
    echo "== Bridge bench sample =="
    echo "BRIDGE_BIN=$BRIDGE_BIN N=$N HTTP=$HTTP GRPC=$GRPC"
    CFG="${CFG:-/tmp/opencode/sn-bench-bridge.toml}"
    HTTP="${HTTP:-http://127.0.0.1:15291}"
    GRPC="${GRPC:-http://127.0.0.1:15261}"
    if [ ! -x "$BRIDGE_BIN" ]; then
      echo "[WARN] BRIDGE_BIN not executable: $BRIDGE_BIN" >&2
    fi
    cat > "$CFG" <<TOML
bridge_address = "$GRPC"
http_address = "$HTTP"

[cluster]
enabled = false
TOML
    : > /tmp/sn-ready.txt
    : > /tmp/sn-health.txt
    : > /tmp/sn-sops.txt
    echo "-- ready_ms (N=$N) --"
    for i in $(seq 1 "$N"); do
      START=$(date +%s%N)
      SISYPHUS_CONFIG="$CFG" SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true "$BRIDGE_BIN" > /tmp/sn-bridge-bench.log 2>&1 &
      BPID=$!
      ready=""
      for j in $(seq 1 100); do
        if curl -sf --max-time 1 "$HTTP/health" >/dev/null 2>&1; then
          READY=$(date +%s%N); ready=$(( (READY-START)/1000000 )); echo "$ready" >> /tmp/sn-ready.txt; break
        fi
        sleep 0.1
      done
      if [ -z "$ready" ]; then
        echo "[WARN] run $i: bridge not ready within timeout" >&2
      else
        curl -o /dev/null -s -w '%{time_total}\n' "$HTTP/health" >> /tmp/sn-health.txt 2>/dev/null || true
        curl -o /dev/null -s -w '%{time_total}\n' "$HTTP/api/sops" >> /tmp/sn-sops.txt 2>/dev/null || true
      fi
      kill "$BPID" 2>/dev/null || true
      wait "$BPID" 2>/dev/null || true
      sleep 0.2
      pkill -f "$BRIDGE_BIN" 2>/dev/null || true
    done
    cat /tmp/sn-ready.txt | stats
    echo "-- /health latency (s) --"; cat /tmp/sn-health.txt 2>/dev/null | stats || true
    echo "-- /api/sops latency (s) --"; cat /tmp/sn-sops.txt 2>/dev/null | stats || true
    ;;
  *)
    echo "Unknown mode: $MODE (use cli|bridge)" >&2
    usage >&2
    exit 2
    ;;
esac
