#!/usr/bin/env bash
# T4.1：质量指标首轮采集脚本。
# 输出 JSON 到 docs/benchmarks/quality-metrics/<YYYY-MM-DD>-<commit>.json
# 来源：
#   - bridge /api/metrics（sisyphus_llm_requests_total / sisyphus_llm_tokens_total），
#     未运行时记 null（不伪造）
#   - 任务级成功率/修复率/回归率：依赖 L3 执行证据，当前为 null（T1.4 BLOCKED）
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/quality-metrics-collect.sh

Collects first-round single-node quality metrics into
docs/benchmarks/quality-metrics/<YYYY-MM-DD>-<commit>.json

Env:
  BRIDGE_HTTP=http://127.0.0.1:15291   # bridge 未运行时采集为 unavailable
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

BRIDGE_HTTP="${BRIDGE_HTTP:-http://127.0.0.1:15291}"
COMMIT="$(git rev-parse --short HEAD)"
STAMP="$(date -u +%Y-%m-%d)"
OUT_DIR="docs/benchmarks/quality-metrics"
OUT="${OUT_DIR}/${STAMP}-${COMMIT}.json"

bridge_up="false"
requests_total="null"
tokens_total="null"
METRICS_JSON=""
if curl -sf --max-time 2 "${BRIDGE_HTTP}/health" >/dev/null 2>&1; then
  bridge_up="true"
  METRICS_JSON="$(curl -sf --max-time 5 ${BRIDGE_HTTP}/metrics)"
  if [ -n "$METRICS_JSON" ]; then
    requests_raw="$(printf '%s\n' "$METRICS_JSON" | grep -E '^sisyphus_llm_requests_total' | tail -n 1 | awk '{print $NF}')"
    tokens_raw="$(printf '%s\n' "$METRICS_JSON" | grep -E '^sisyphus_llm_tokens_total' | tail -n 1 | awk '{print $NF}')"
    requests_total="${requests_raw:-null}"
    tokens_total="${tokens_raw:-null}"
  fi
fi

python3 - "$BRIDGE_HTTP" "$COMMIT" "$STAMP" "$bridge_up" "$requests_total" "$tokens_total" <<'PY'
import datetime
import json
import os

bridge_http, commit, stamp, bridge_up, requests_total, tokens_total = (
    __import__("sys").argv[1:7]
)
metrics = {
    "collected_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "commit": commit,
    "bridge_http": bridge_http,
    "bridge_metrics_available": bridge_up == "true",
    "llm_requests_total": int(requests_total) if requests_total not in (None, "null") else None,
    "llm_tokens_total": int(tokens_total) if tokens_total not in (None, "null") else None,
    # 任务级指标（成功率/修复率/回归率）：依赖 L3 执行证据，当前阶段按口径记为 null。
    "task_success_rate": None,
    "task_fix_rate": None,
    "task_regression_rate": None,
    "task_cost_duration_ms_per_task": None,
}
out_dir = "docs/benchmarks/quality-metrics"
os.makedirs(out_dir, exist_ok=True)
out_path = f"{out_dir}/{stamp}-{commit}.json"
with open(out_path, "w") as f:
    json.dump(metrics, f, indent=2)
    f.write("\n")
print(json.dumps(metrics, indent=2))
PY

printf 'WROTE %s\n' "$OUT"
