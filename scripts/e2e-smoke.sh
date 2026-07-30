#!/usr/bin/env bash
# E2E smoke test: start bridge → run stress_test --quick → verify response
set -euo pipefail

# Check that binaries are already built (by Makefile prerequisite)
if [ ! -f target/debug/sisyphus-bridge ]; then
    echo "ERROR: sisyphus-bridge binary not found. Run 'cargo build -p sisyphus-bridge' first."
    exit 1
fi
if [ ! -f target/debug/stress_test ]; then
    echo "ERROR: stress_test binary not found. Run 'cargo build -p sisyphus-bridge --bin stress_test' first."
    exit 1
fi

echo "━━━ Starting bridge in background ━━━"
./target/debug/sisyphus-bridge &
BRIDGE_PID=$!

# Trap to clean up bridge process on exit
cleanup() {
    echo "━━━ Cleaning up bridge (PID $BRIDGE_PID) ━━━"
    kill "$BRIDGE_PID" 2>/dev/null || true
    wait "$BRIDGE_PID" 2>/dev/null || true
}
trap cleanup EXIT

# Wait for bridge to be ready by polling with stress_test as health check
# gRPC doesn't support plain curl, so we use the stress_test binary itself.
echo "━━━ Waiting for bridge to start ━━━"
for i in $(seq 1 10); do
    if ./target/debug/stress_test --quick 2>/dev/null; then
        echo "Bridge ready on attempt $i"
        break
    fi
    if [ "$i" -eq 10 ]; then
        echo "ERROR: Bridge did not start within 10 seconds"
        exit 1
    fi
    sleep 1
done

echo "━━━ Running stress_test --quick ━━━"
./target/debug/stress_test --quick
echo "━━━ E2E smoke test passed ━━━"
