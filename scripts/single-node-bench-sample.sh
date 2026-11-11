#!/usr/bin/env bash
# P2-B: 单机 CLI/bridge 轻量采样脚本（多次取样，输出摘要）
# 用法:
#   ./scripts/single-node-bench-sample.sh cli
#   ./scripts/single-node-bench-sample.sh bridge
#   ./scripts/single-node-bench-sample.sh --help
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/single-node-bench-sample.sh <cli|bridge> [--json]

Modes:
  cli     对 debug/release CLI 常用命令做 N 次采样并输出 min/p50/p95/avg/max
  bridge  对 bridge 启动就绪与只读端点延迟做 N 次采样并输出 min/p50/p95/avg/max

Options:
  --json  输出机器可读 JSON（含 timestamp/build 与分位数，便于对比/归档）

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
JSON_OUT=false
shift || true
while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON_OUT=true ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

N="${N:-5}"
CLI_BIN="${CLI_BIN:-./target/debug/sisyphus}"
BRIDGE_BIN="${BRIDGE_BIN:-./target/debug/sisyphus-bridge}"

# 采样统计：排序后计算 nearest-rank p50/p95（n 较小时 p95 可能等于 max，属预期）。
stats() {
  local tmp
  tmp=$(mktemp)
  sort -n > "$tmp"
  awk '
    function pct(v, n, q,   r) {
      r = int(q * n);
      if (q * n > r) r++;
      if (r < 1) r = 1;
      if (r > n) r = n;
      return v[r];
    }
    { v[NR] = $1 + 0; sum += v[NR] }
    END {
      n = NR;
      if (n == 0) { print "n=0"; exit }
      printf "n=%d min=%.6f p50=%.6f p95=%.6f avg=%.6f max=%.6f\n",
        n, v[1], pct(v, n, 0.50), pct(v, n, 0.95), sum / n, v[n];
    }' "$tmp"
  rm -f "$tmp"
}

stats_json() {
  local tmp
  tmp=$(mktemp)
  sort -n > "$tmp"
  awk '
    function pct(v, n, q,   r) {
      r = int(q * n);
      if (q * n > r) r++;
      if (r < 1) r = 1;
      if (r > n) r = n;
      return v[r];
    }
    { v[NR] = $1 + 0; sum += v[NR] }
    END {
      n = NR;
      if (n == 0) {
        print "{\"n\":0,\"min\":null,\"p50\":null,\"p95\":null,\"avg\":null,\"max\":null}";
        exit;
      }
      printf "{\"n\":%d,\"min\":%.6f,\"p50\":%.6f,\"p95\":%.6f,\"avg\":%.6f,\"max\":%.6f}",
        n, v[1], pct(v, n, 0.50), pct(v, n, 0.95), sum / n, v[n];
    }' "$tmp"
  rm -f "$tmp"
}

build_kind() {
  case "$1" in
    */release/*) echo "release" ;;
    */debug/*) echo "debug" ;;
    *) echo "unknown" ;;
  esac
}

TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

case "$MODE" in
  cli)
    if [ "$JSON_OUT" = false ]; then
      echo "== CLI bench sample =="
      echo "CLI_BIN=$CLI_BIN N=$N"
    fi
    if [ ! -x "$CLI_BIN" ]; then
      echo "[WARN] CLI_BIN not executable: $CLI_BIN" >&2
    fi
    mkdir -p /tmp/opencode/bench-sn
    CLI_BUILD="$(build_kind "$CLI_BIN")"
    if [ "$JSON_OUT" = true ]; then
      echo "{"
      echo "  \"mode\": \"cli\","
      echo "  \"timestamp\": \"${TIMESTAMP}\","
      echo "  \"build\": \"${CLI_BUILD}\","
      echo "  \"cli_bin\": \"${CLI_BIN}\","
      echo "  \"n\": ${N},"
      echo "  \"commands\": ["
    fi
    first_cmd=1
    for cmd in "--help" "exec echo ok" "--data-dir /tmp/opencode/bench-sn session list"; do
      : > /tmp/sn-cli-samples.txt
      if [ "$JSON_OUT" = false ]; then
        echo "-- sisyphus $cmd --"
      fi
      for i in $(seq 1 "$N"); do
        # shellcheck disable=SC2086
        if command -v python3 >/dev/null 2>&1; then
          t=$(python3 - "$CLI_BIN $cmd" <<'PY'
import subprocess, sys, time
cmd = sys.argv[1]
t0 = time.perf_counter()
subprocess.run(cmd, shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
print(f"{time.perf_counter() - t0:.6f}")
PY
)
        else
          t=$(/usr/bin/time -p $CLI_BIN $cmd >/dev/null 2> /tmp/sn-time.err; awk '/^real/{print $2}' /tmp/sn-time.err)
        fi
        echo "$t" >> /tmp/sn-cli-samples.txt
      done
      if [ "$JSON_OUT" = true ]; then
        statsj=$(cat /tmp/sn-cli-samples.txt | stats_json)
        # escape cmd for JSON
        cmd_esc=$(printf '%s' "$cmd" | sed 's/\\/\\\\/g; s/"/\\"/g')
        if [ $first_cmd -eq 1 ]; then
          printf '    {"cmd": "sisyphus %s", "stats": %s}' "$cmd_esc" "$statsj"
        else
          printf ',\n    {"cmd": "sisyphus %s", "stats": %s}' "$cmd_esc" "$statsj"
        fi
        first_cmd=0
      else
        cat /tmp/sn-cli-samples.txt | stats
      fi
    done
    if [ "$JSON_OUT" = true ]; then
      printf '\n  ]\n}\n'
    fi
    ;;
  bridge)
    if [ "$JSON_OUT" = false ]; then
      echo "== Bridge bench sample =="
      echo "BRIDGE_BIN=$BRIDGE_BIN N=$N HTTP=$HTTP GRPC=$GRPC"
    fi
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
    if [ "$JSON_OUT" = false ]; then
      echo "-- ready_ms (N=$N) --"
    fi
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
    if [ "$JSON_OUT" = true ]; then
      BRIDGE_BUILD="$(build_kind "$BRIDGE_BIN")"
      readyj=$(cat /tmp/sn-ready.txt | stats_json)
      healthj=$(cat /tmp/sn-health.txt 2>/dev/null | stats_json || echo '{"n":0,"min":null,"p50":null,"p95":null,"avg":null,"max":null}')
      sopsj=$(cat /tmp/sn-sops.txt 2>/dev/null | stats_json || echo '{"n":0,"min":null,"p50":null,"p95":null,"avg":null,"max":null}')
      printf '{"mode":"bridge","timestamp":"%s","build":"%s","bridge_bin":"%s","n":%s,"http":"%s","grpc":"%s","ready_ms":%s,"health_s":%s,"sops_s":%s}\n' \
        "$TIMESTAMP" "$BRIDGE_BUILD" "$BRIDGE_BIN" "$N" "$HTTP" "$GRPC" "$readyj" "$healthj" "$sopsj"
    else
      cat /tmp/sn-ready.txt | stats
      echo "-- /health latency (s) --"; cat /tmp/sn-health.txt 2>/dev/null | stats || true
      echo "-- /api/sops latency (s) --"; cat /tmp/sn-sops.txt 2>/dev/null | stats || true
    fi
    ;;
  *)
    echo "Unknown mode: $MODE (use cli|bridge)" >&2
    usage >&2
    exit 2
    ;;
esac
