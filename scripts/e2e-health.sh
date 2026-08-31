#!/usr/bin/env bash
# E2E smoke test for /health/deep endpoint.
#
# Flow:
#   1. Start bridge in background
#   2. Wait for gRPC readiness (via stress_test --quick)
#   3. curl GET /health/deep
#   4. Verify response: 200 OK + JSON with `components` map
#   5. Verify at least `kernel` and `session_db` keys present
#   6. Shutdown bridge
#
# Usage:
#   cargo build -p sisyphus-bridge
#   ./scripts/e2e-health.sh
#
# Exits 0 on success, non-zero on any failure.

set -euo pipefail

# ── Configuration ──────────────────────────────────────────────────────
HTTP_HOST="${SISYPHUS_HTTP_HOST:-127.0.0.1}"
HTTP_PORT="${SISYPHUS_HTTP_PORT:-8080}"
HEALTH_URL="http://${HTTP_HOST}:${HTTP_PORT}/health"
HEALTH_DEEP_URL="http://${HTTP_HOST}:${HTTP_PORT}/health/deep"
GRPC_HEALTH_TIMEOUT_SECS=10

# ── Pre-flight checks ──────────────────────────────────────────────────
if [ ! -f target/debug/sisyphus-bridge ]; then
    echo "ERROR: sisyphus-bridge binary not found."
    echo "Run 'cargo build -p sisyphus-bridge' first."
    exit 1
fi
if [ ! -f target/debug/stress_test ]; then
    echo "ERROR: stress_test binary not found."
    echo "Run 'cargo build -p sisyphus-bridge --bin stress_test' first."
    exit 1
fi
if ! command -v curl >/dev/null 2>&1; then
    echo "ERROR: curl is required but not installed."
    exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
    echo "ERROR: python3 is required (for JSON parsing) but not installed."
    exit 1
fi

# ── Start bridge ───────────────────────────────────────────────────────
echo "━━━ Starting bridge in background ━━━"
./target/debug/sisyphus-bridge > /tmp/sisyphus-bridge-e2e.log 2>&1 &
BRIDGE_PID=$!

cleanup() {
    echo "━━━ Cleaning up bridge (PID $BRIDGE_PID) ━━━"
    kill "$BRIDGE_PID" 2>/dev/null || true
    wait "$BRIDGE_PID" 2>/dev/null || true
}
trap cleanup EXIT

# ── Wait for gRPC readiness via stress_test ────────────────────────────
echo "━━━ Waiting for bridge readiness (max ${GRPC_HEALTH_TIMEOUT_SECS}s) ━━━"
for i in $(seq 1 "$GRPC_HEALTH_TIMEOUT_SECS"); do
    if ./target/debug/stress_test --quick >/dev/null 2>&1; then
        echo "Bridge ready on attempt ${i}s"
        break
    fi
    if [ "$i" -eq "$GRPC_HEALTH_TIMEOUT_SECS" ]; then
        echo "ERROR: Bridge did not start within ${GRPC_HEALTH_TIMEOUT_SECS} seconds"
        echo "─── bridge log tail ───"
        tail -20 /tmp/sisyphus-bridge-e2e.log
        exit 1
    fi
    sleep 1
done

# ── Test 1: /health (simple 200) ────────────────────────────────────────
echo ""
echo "━━━ Test 1: GET /health (simple) ━━━"
HEALTH_STATUS=$(curl -sS -o /tmp/health-body.txt -w "%{http_code}" "$HEALTH_URL")
if [ "$HEALTH_STATUS" != "200" ]; then
    echo "ERROR: /health returned HTTP $HEALTH_STATUS (expected 200)"
    cat /tmp/health-body.txt
    exit 1
fi
echo "✓ /health → 200 OK"
cat /tmp/health-body.txt
echo ""

# ── Test 2: /health/deep (200 或 207 Multi-Status) ─────────────────────
echo "━━━ Test 2: GET /health/deep (per-component) ━━━"
HEALTH_DEEP_STATUS=$(curl -sS -o /tmp/health-deep-body.txt -w "%{http_code}" "$HEALTH_DEEP_URL")
case "$HEALTH_DEEP_STATUS" in
    200|207)
        echo "✓ /health/deep → $HEALTH_DEEP_STATUS (OK or Multi-Status)"
        ;;
    *)
        echo "ERROR: /health/deep returned HTTP $HEALTH_DEEP_STATUS (expected 200 or 207)"
        cat /tmp/health-deep-body.txt
        exit 1
        ;;
esac

# ── Test 3: 验证 JSON 结构（必须含 components 键）──
echo "━━━ Test 3: Validate JSON structure ━━━"
if ! python3 -c "
import json, sys
with open('/tmp/health-deep-body.txt') as f:
    body = json.load(f)
assert 'status' in body, 'missing top-level status'
assert 'components' in body, 'missing components map'
components = body['components']
required = {'session_db', 'shared_http', 'kernel'}
missing = required - set(components.keys())
assert not missing, f'missing components: {missing}'
for name, info in components.items():
    assert 'status' in info, f'{name} missing status field'
    assert info['status'] in ('ok', 'degraded', 'absent'), f'{name} has invalid status: {info[\"status\"]}'
print(f'  ✓ status = {body[\"status\"]}')
print(f'  ✓ components = {sorted(components.keys())}')
for name in sorted(components):
    print(f'    - {name}: {components[name][\"status\"]}')
" 2>&1; then
    echo "ERROR: /health/deep response does not match expected schema"
    cat /tmp/health-deep-body.txt
    exit 1
fi

echo ""
echo "━━━ E2E health test passed ━━━"
