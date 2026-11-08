#!/usr/bin/env bash
# ADR-019：控制面 mTLS 证据采集流水线（loopback / 单进程，非专用 runner 真实网络证据）。
#
# 流程：
#   1. 生成测试 PKI（rcgen；`make gen-control-plane-test-certs` / scripts/gen...sh）；
#   2. 生成临时 bridge 配置（`[cluster.control_plane]` enabled + tls_enabled + mTLS allowlist）；
#   3. 启动 bridge（TLS 应用于共享 gRPC 端口）；
#   4. 用 mTLS 跑 `control_plane_harness`（submit + get）并输出 JSON 证据；
#   5. 汇总证据路径。
#
# 边界（不得外推）：本脚本验证的是 **loopback + 单进程** 的真实 TLS/mTLS gRPC 链路；
# ADR-019 专用 runner 的真实跨进程网络分区/混沌/重启/TLS 长稳证据仍需专用运行环境执行。

set -euo pipefail

OUT_DIR="${1:-./control-plane-certs}"
BRIDGE_ADDR="http://127.0.0.1:50051"
NODES=()
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT_DIR="$2"; shift 2 ;;
    --node) NODES+=("$2"); shift 2 ;;
    --addr) BRIDGE_ADDR="$2"; shift 2 ;;
    -h|--help)
      echo "用法: $0 [--out DIR] [--node NAME ...] [--addr http://127.0.0.1:50051]"
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
CERT_BIN="$(command -v cargo >/dev/null 2>&1 && echo cargo)"
[ -x "$REPO_ROOT/target/debug/sisyphus-bridge" ] || cargo build -p sisyphus-bridge --no-default-features >/dev/null

CERT_ARGS=(--out "$OUT_DIR")
for n in "${NODES[@]}"; do
  CERT_ARGS+=(--node "$n")
done
"$CERT_SCRIPT" "${CERT_ARGS[@]}"

ISSUER="sisyphus-control-plane-evidence"
CFG="$OUT_DIR/bridge-evidence-$ISSUER.toml"
cat >"$CFG" <<EOF
[cluster]
enabled = true
node_id = "${NODES[0]}"
cluster_name = "sisyphus-control-plane-evidence"

[cluster.control_plane]
enabled = true
tls_enabled = true
tls_ca_cert_path = "${OUT_DIR}/ca.pem"
tls_client_cert_path = "${OUT_DIR}/${NODES[0]}.pem"
tls_client_key_path = "${OUT_DIR}/${NODES[0]}.key"
tls_domain_name = "localhost"
submit_auth_mode = "mtls_subject_allowlist"
allowed_submit_subjects = [$(printf '"DNS:%s",' "${NODES[@]}" | sed 's/,$//')]
EOF

BRIDGE_BIN="$REPO_ROOT/target/debug/sisyphus-bridge"
BRIDGE_LOG="$OUT_DIR/bridge-evidence-$ISSUER.log"
echo "==> starting bridge: $BRIDGE_BIN (SISYPHUS_CONFIG=$CFG)"
export SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true
SISYPHUS_CONFIG="$CFG" "$BRIDGE_BIN" >"$BRIDGE_LOG" 2>&1 &
BRIDGE_PID=$!
cleanup() {
  kill "$BRIDGE_PID" 2>/dev/null || true
  wait "$BRIDGE_PID" 2>/dev/null || true
}
trap cleanup EXIT
wait_ready() {
  local i host port
  host="${BRIDGE_ADDR#*://}"
  port="${host##*:}"
  host="${host%%:*}"
  for i in $(seq 1 60); do
    if nc -z "$host" "$port" 2>/dev/null; then
      return 0
    fi
    if nc -z "[::1]" "$port" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  return 1
}
if ! wait_ready; then
  echo "bridge did not become ready on 127.0.0.1:50051; logs: $BRIDGE_LOG" >&2
  exit 1
fi

TS="$(date +%Y%m%dT%H%M%S)"
LEASE_KEY="lease:tls-evidence-$TS"
echo "==> mTLS harness submit (addr=$BRIDGE_ADDR)"
cargo run -p sisyphus-bridge --no-default-features --bin control_plane_harness -- \
  --addr "$BRIDGE_ADDR" \
  --ca-cert "$OUT_DIR/ca.pem" \
  --client-cert "$OUT_DIR/${NODES[0]}.pem" \
  --client-key "$OUT_DIR/${NODES[0]}.key" \
  --domain localhost \
  --key "$LEASE_KEY" \
  --owner "${NODES[0]}" \
  --operation lease.acquire \
  --ttl-secs 30 \
  --output "$OUT_DIR/harness-submit-$TS.json"

echo "==> mTLS harness get (addr=$BRIDGE_ADDR)"
cargo run -p sisyphus-bridge --no-default-features --bin control_plane_harness -- \
  --addr "$BRIDGE_ADDR" \
  --ca-cert "$OUT_DIR/ca.pem" \
  --client-cert "$OUT_DIR/${NODES[0]}.pem" \
  --client-key "$OUT_DIR/${NODES[0]}.key" \
  --domain localhost \
  --get "$LEASE_KEY" \
  --output "$OUT_DIR/harness-get-$TS.json"

cleanup
trap - EXIT
echo "✅ TLS evidence JSONs written under $OUT_DIR"
