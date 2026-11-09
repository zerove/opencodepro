#!/usr/bin/env bash
# ADR-019：控制面 mTLS N=3 证据采集流水线（loopback / 多节点，非专用 runner 真实网络证据）。
#
# 流程：
#   1. 生成测试 PKI（rcgen；`make gen-control-plane-test-certs` / scripts/gen...sh）；
#   2. 为每个节点生成 bridge 配置（cluster.enabled + control_plane.tls_enabled + mTLS
#      allowlist + static peers）；
#   3. 启动 N 个 bridge（各自监听 gRPC TLS 端口；TLS 由 control_plane 装配生效）；
#   4. mTLS harness submit/get（按节点）并输出 JSON 证据；
#   5. 汇总证据路径。
#
# 用法: scripts/run-control-plane-tls-evidence-n3.sh [--out DIR] [--node NAME ...]
#   （默认节点集 node-1 node-2 node-3；默认 OUT_DIR ./control-plane-certs-n3）
#
# 边界（不得外推）：本脚本验证的是 **loopback + 单主机多进程** 的真实 TLS/mTLS N=3 多节点
# gRPC 链路；ADR-019 专用 runner 的真实网络分区/混沌/TLS 长稳/重启证据仍需专用运行环境执行。

set -euo pipefail

OUT_DIR="${1:-./control-plane-certs-n3}"
BASE_PORT="${BASE_PORT:-50051}"
NODES=()
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT_DIR="$2"; shift 2 ;;
    --node) NODES+=("$2"); shift 2 ;;
    -h|--help)
      echo "用法: $0 [--out DIR] [--node NAME ...]"
      exit 0
      ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done
if [ ${#NODES[@]} -eq 0 ]; then
  NODES=(node-1 node-2 node-3)
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CERT_SCRIPT="$REPO_ROOT/scripts/gen-control-plane-test-certs.sh"
[ -x "$REPO_ROOT/target/debug/sisyphus-bridge" ] || cargo build -p sisyphus-bridge --no-default-features >/dev/null

CERT_ARGS=(--out "$OUT_DIR")
for n in "${NODES[@]}"; do
  CERT_ARGS+=(--node "$n")
done
"$CERT_SCRIPT" "${CERT_ARGS[@]}"

BRIDGE_BIN="$REPO_ROOT/target/debug/sisyphus-bridge"
BRIDGE_PIDS=()
BRIDGE_LOGS=()
BRIDGE_ADDRS=()

cleanup() {
  local pid
  for pid in ${BRIDGE_PIDS[@]:-}; do
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
}
trap cleanup EXIT

start_bridge() {
  local node="$1"
  local port="$2"
  local idx="$3"
  local cfg="$OUT_DIR/bridge-$node.toml"
  local peers_toml="["
  local first=1
  local j
  local addr
  for j in "${!NODES[@]}"; do
    if [ "$j" -ne "$idx" ]; then
      addr="http://127.0.0.1:$((BASE_PORT + j))"
      if [ "$first" -eq 1 ]; then first=0; else peers_toml+=", "; fi
      peers_toml+="\"$addr\""
    fi
  done
  peers_toml+="]"
  cat >"$cfg" <<EOF
[cluster]
enabled = true
node_id = "$node"
cluster_name = "sisyphus-control-plane-evidence"
peers = $peers_toml

[cluster.control_plane]
enabled = true
tls_enabled = true
tls_ca_cert_path = "$OUT_DIR/ca.pem"
tls_client_cert_path = "$OUT_DIR/$node.pem"
tls_client_key_path = "$OUT_DIR/$node.key"
tls_domain_name = "$node"
submit_auth_mode = "mtls_subject_allowlist"
allowed_submit_subjects = ["DNS:node-1","DNS:node-2","DNS:node-3"]

EOF
  export SISYPHUS_CONFIG="$cfg"
  export SISYPHUS_BRIDGE_ADDRESS="http://127.0.0.1:$port"
  export SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true
  "$BRIDGE_BIN" >"$OUT_DIR/bridge-$node.log" 2>&1 &
  local pid=$!
  BRIDGE_PIDS+=("$pid")
  BRIDGE_LOGS+=("$OUT_DIR/bridge-$node.log")
  BRIDGE_ADDRS+=("https://127.0.0.1:$port")
}

i=0
for n in "${NODES[@]}"; do
  start_bridge "$n" "$((BASE_PORT + i))" "$i"
  i=$((i + 1))
done

wait_ready() {
  local k
  for k in "${!BRIDGE_ADDRS[@]}"; do
    local addr="${BRIDGE_ADDRS[$k]}"
    local port="${addr#*://}"
    port="${port##*:}"
    port="${port%%/*}"
    local tries=0
    until nc -z "127.0.0.1" "$port" 2>/dev/null; do
      tries=$((tries + 1))
      if [ "$tries" -ge 90 ]; then
        echo "bridge ${NODES[$k]} did not become ready on 127.0.0.1:$port" >&2
        return 1
      fi
      sleep 1
    done
  done
  return 0
}
if ! wait_ready; then
  echo "N=3 bridges did not become ready; logs: ${BRIDGE_LOGS[*]}" >&2
  exit 1
fi

LEADER="${NODES[0]}"
k=0
for n in "${NODES[@]}"; do
  addr="${BRIDGE_ADDRS[$k]}"
  port="${addr#*://}"
  port="${port%%/*}"
  ts="n3-$(basename "$n")"
  echo "==> mTLS N=3 harness submit (node=$n addr=$addr)"
  if [ "$n" = "$LEADER" ]; then
    cargo run -p sisyphus-bridge --no-default-features --bin control_plane_harness -- \
      --addr "$addr" \
      --ca-cert "$OUT_DIR/ca.pem" \
      --client-cert "$OUT_DIR/$n.pem" \
      --client-key "$OUT_DIR/$n.key" \
      --domain "$n" \
      --key "lease:$ts" \
      --owner "$n" \
      --operation lease.acquire \
      --ttl-secs 30 \
      --output "$OUT_DIR/harness-n3-submit-$n-$ts.json"
    cargo run -p sisyphus-bridge --no-default-features --bin control_plane_harness -- \
      --addr "$addr" \
      --ca-cert "$OUT_DIR/ca.pem" \
      --client-cert "$OUT_DIR/$n.pem" \
      --client-key "$OUT_DIR/$n.key" \
      --domain "$n" \
      --get "lease:$ts" \
      --output "$OUT_DIR/harness-n3-get-$n-$ts.json"
  else
    cargo run -p sisyphus-bridge --no-default-features --bin control_plane_harness -- \
      --addr "$addr" \
      --ca-cert "$OUT_DIR/ca.pem" \
      --client-cert "$OUT_DIR/$n.pem" \
      --client-key "$OUT_DIR/$n.key" \
      --domain "$n" \
      --key "lease:$ts" \
      --owner "$n" \
      --operation lease.acquire \
      --ttl-secs 30 \
      --output "$OUT_DIR/harness-n3-submit-$n-$ts.json"
  fi
  k=$((k + 1))
done

cleanup
trap - EXIT
echo "✅ N=3 TLS evidence JSONs written under $OUT_DIR"
