#!/usr/bin/env bash
# P2-C: 单机就绪与只读观测检查（可运行样例）
# 用法: BASE=http://127.0.0.1:8080 WS=default ./scripts/single-node-readiness-check.sh
set -euo pipefail

BASE="${BASE:-http://127.0.0.1:8080}"
WS="${WS:-default}"
TIMEOUT="${TIMEOUT:-5}"

pass() { printf '[PASS] %s\n' "$*"; }
fail() { printf '[FAIL] %s\n' "$*" >&2; exit 1; }

echo "== Single-node readiness check =="
echo "BASE=$BASE WS=$WS TIMEOUT=$TIMEOUT"

if curl -fsS --max-time "$TIMEOUT" "$BASE/health" >/tmp/sn-health.json; then
  pass "health reachable ($(tr -d '\n' </tmp/sn-health.json | head -c 120))"
else
  fail "health unreachable ($BASE/health)"
fi

if curl -fsS --max-time "$TIMEOUT" "$BASE/api/sops" >/tmp/sn-sops.json; then
  pass "sops list: $(tr -d '\n' </tmp/sn-sops.json | head -c 120)"
else
  fail "sops endpoint failed"
fi

if curl -fsS --max-time "$TIMEOUT" "$BASE/api/cluster/status" >/tmp/sn-cluster.json; then
  # best-effort summary without jq dependency
  summary=$(tr -d '\n' </tmp/sn-cluster.json | head -c 160)
  pass "cluster status (readonly): ${summary}..."
else
  fail "cluster status failed"
fi

echo "== OK =="
