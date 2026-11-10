#!/usr/bin/env bash
# 保存并可选执行 SOP 模板（单机场景辅助）
# 用法:
#   BASE=http://127.0.0.1:8080 BRIDGE_GRPC=http://127.0.0.1:50051 WS=default \
#     ./scripts/single-node-apply-sop-template.sh docs/examples/sop-templates/verify-sequence.json [--run]
set -euo pipefail

BASE="${BASE:-http://127.0.0.1:8080}"
BRIDGE_GRPC="${BRIDGE_GRPC:-http://127.0.0.1:50051}"
WS="${WS:-default}"
CLI="${CLI:-}"
TIMEOUT="${TIMEOUT:-10}"

if [ $# -lt 1 ]; then
  echo "Usage: $0 <template.json> [--run]" >&2
  exit 2
fi
TEMPLATE="$1"
RUN="${2:-}"

if [ -z "$CLI" ]; then
  if [ -x "./target/debug/sisyphus" ]; then CLI="./target/debug/sisyphus";
  elif [ -x "./target/release/sisyphus" ]; then CLI="./target/release/sisyphus";
  else CLI="sisyphus"; fi
fi

if [ ! -f "$TEMPLATE" ]; then
  echo "[FAIL] template not found: $TEMPLATE" >&2
  exit 1
fi

SOP_ID=$(grep -o '"id"[[:space:]]*:[[:space:]]*"[^"]*"' "$TEMPLATE" | head -1 | sed 's/.*"id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')
if [ -z "$SOP_ID" ]; then
  echo "[FAIL] cannot parse sop id from template: $TEMPLATE" >&2
  exit 1
fi

echo "== save template =="
echo "template=$TEMPLATE sop_id=$SOP_ID ws=$WS base=$BASE"
code=$(curl -sS -o /tmp/sn-save-sop.out -w '%{http_code}' --max-time "$TIMEOUT" -X POST "$BASE/api/sops" \
  -H 'Content-Type: application/json' \
  -H "X-Sisyphus-Workspace-Id: $WS" \
  --data-binary @"$TEMPLATE")
echo "http_status=$code"
cat /tmp/sn-save-sop.out 2>/dev/null || true
echo
if [ "$code" != "200" ] && [ "$code" != "201" ]; then
  echo "[FAIL] save failed (expected 200/201)" >&2
  exit 1
fi

echo "== verify detail =="
curl -fsS --max-time "$TIMEOUT" -H "X-Sisyphus-Workspace-Id: $WS" "$BASE/api/sops/$SOP_ID" | head -c 400; echo

if [ "$RUN" = "--run" ]; then
  echo "== run sop (CLI direct path) =="
  set +e
  "$CLI" --bridge "$BRIDGE_GRPC" sop run "$SOP_ID"
  rc=$?
  set -e
  echo "sop_run_exit_code=$rc"
  if [ "$rc" -ne 0 ]; then
    echo "[WARN] sop run failed (exit=$rc). 检查 bridge 地址、SOP/执行器可用性与依赖；目标驱动路径可尝试 POST /api/execute/goal（可能依赖 LLM）。" >&2
  fi
fi

echo "== done =="
