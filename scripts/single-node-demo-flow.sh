#!/usr/bin/env bash
# P1-C/P1-E/P2-C: 单机端到端演示流程（发现 -> 详情 -> 执行 -> 观测）
# 适配 Sisyphus 现有契约：优先展示可审计 SOP/DAG 路径与可观测端点。
#
# 用法:
#   BASE=http://127.0.0.1:8080 BRIDGE_GRPC=http://127.0.0.1:50051 SOP_ID=verify_sop \
#     ./scripts/single-node-demo-flow.sh
#
# 前提: bridge HTTP/gRPC 已启动且可达；CLI 可用（默认 target/debug/sisyphus 或 PATH 中 sisyphus）。
set -euo pipefail

BASE="${BASE:-http://127.0.0.1:8080}"
BRIDGE_GRPC="${BRIDGE_GRPC:-http://127.0.0.1:50051}"
WS="${WS:-default}"
SOP_ID="${SOP_ID:-verify_sop}"
CLI="${CLI:-}"

if [ -z "$CLI" ]; then
  if [ -x "./target/debug/sisyphus" ]; then CLI="./target/debug/sisyphus";
  elif [ -x "./target/release/sisyphus" ]; then CLI="./target/release/sisyphus";
  else CLI="sisyphus"; fi
fi

hr() { printf '\n== %s ==\n' "$*"; }

hr "1) 就绪检查（HTTP 只读面）"
BASE="$BASE" WS="$WS" ./scripts/single-node-readiness-check.sh

hr "2) SOP 发现与详情（REST）"
echo "-- GET /api/sops --"
curl -fsS "$BASE/api/sops"; echo
echo "-- GET /api/sops/$SOP_ID --"
curl -fsS -H "X-Sisyphus-Workspace-Id: $WS" "$BASE/api/sops/$SOP_ID"; echo

hr "3) SOP 执行（CLI 直接路径，经 gRPC bridge）"
echo "CLI=$CLI BRIDGE_GRPC=$BRIDGE_GRPC SOP_ID=$SOP_ID"
set +e
"$CLI" --bridge "$BRIDGE_GRPC" sop run "$SOP_ID"
RC=$?
set -e
echo "exit_code=$RC"
if [ "$RC" -ne 0 ]; then
  echo "提示: 若失败，请检查 bridge gRPC 地址、SOP 是否存在、执行器权限；execute/goal 路径与 sop run 路径不同（见 runbook）。" >&2
fi

hr "4) 观测入口（只读）"
echo "cluster_status: $BASE/api/cluster/status"
curl -fsS "$BASE/api/cluster/status" | head -c 240; echo
echo "sop_dag: $BASE/api/sops/$SOP_ID/dag (若可用)"
curl -fsS "$BASE/api/sops/$SOP_ID/dag" | head -c 240 2>/dev/null || echo "(dag endpoint unavailable or requires context)"

hr "完成"
echo "说明: 本流程验证端到端可达性、可发现性与直接 SOP 执行路径；POST /api/execute/goal 为目标驱动路径，可能依赖 LLM 配置。"
