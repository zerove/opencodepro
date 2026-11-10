#!/usr/bin/env bash
# P2-B: 单机 CLI/bridge 轻量采样脚本（多次取样，输出摘要）
# 用法:
#   ./scripts/single-node-bench-sample.sh cli
#   BRIDGE_BIN=./target/debug/sisyphus-bridge BASE_CFG=/tmp/sn.toml ./scripts/single-node-bench-sample.sh bridge
set -euo pipefail

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
    for cmd in "--help" "exec echo ok" "--data-dir /tmp/opencode/bench-sn session list"; do
      echo "-- sisyphus $cmd --"
      : > /tmp/sn-bench-times.txt
      for i in $(seq 1 "$N"); do
        # shellcheck disable=SC2086
        t=$(/usr/bin/time -p $CLI_BIN $cmd >/dev/null 2> /tmp/sn-time.err; awk '/^real/{print $2}' /tmp/sn-time.err)
        echo "$t"
      done | tee /tmp/sn-bench-raw.txt | stats
    done
    ;;
  bridge)
    echo "== Bridge bench sample =="
    echo "BRIDGE_BIN=$BRIDGE_BIN N=$N (requires jq not needed; uses curl timings)"
    CFG="${CFG:-/tmp/opencode/sn-bench-bridge.toml}"
    HTTP="${HTTP:-http://127.0.0.1:15291}"
    GRPC="${GRPC:-http://127.0.0.1:15261}"
    cat > "$CFG" <<TOML
bridge_address = "$GRPC"
http_address = "$HTTP"

[cluster]
enabled = false
TOML
    echo "-- ready_ms (N=$N) --"
    : > /tmp/sn-ready.txt
    for i in $(seq 1 "$N"); do
      START=$(date +%s%N)
      SISYPHUS_CONFIG="$CFG" SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true "$BRIDGE_BIN" > /tmp/sn-bridge-bench.log 2>&1 &
      BPID=$!
      for j in $(seq 1 100); do
        if curl -sf --max-time 1 "$HTTP/health" >/dev/null 2>&1; then
          READY=$(date +%s%N); echo $(( (READY-START)/1000000 )) >> /tmp/sn-ready.txt; break
        fi
        sleep 0.1
      done
      # sample endpoint latencies once per run
      curl -o /dev/null -s -w '%{time_total}\n' "$HTTP/health" >> /tmp/sn-health.txt 2>/dev/null || true
      curl -o /dev/null -s -w '%{time_total}\n' "$HTTP/api/sops" >> /tmp/sn-sops.txt 2>/dev/null || true
      kill $BPID 2>/dev/null; wait $BPID 2>/dev/null || true
      sleep 0.2
      pkill -f "$BRIDGE_BIN" 2>/dev/null || true
    done
    cat /tmp/sn-ready.txt | stats
    echo "-- /health latency (s) --"; cat /tmp/sn-health.txt 2>/dev/null | stats || true
    echo "-- /api/sops latency (s) --"; cat /tmp/sn-sops.txt 2>/dev/null | stats || true
    ;;
  *)
    echo "Unknown mode: $MODE (use cli|bridge)" >&2; exit 2 ;;
esac
