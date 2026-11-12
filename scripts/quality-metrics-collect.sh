#!/usr/bin/env bash
# T4.1：质量指标采集脚本。
#
# 输出 JSON 到 docs/benchmarks/quality-metrics/<YYYY-MM-DD>-<commit>.json
# 来源：
#   - bridge /api/metrics（sisyphus_llm_requests_total / sisyphus_llm_tokens_total），
#     未运行时记 null（不伪造）
#   - 任务级指标（成功率/修复率/回归率/单任务耗时/token 成本）：由
#     `sisyphus metrics quality` 从任务记录目录聚合（记录由
#     `sisyphus verify --record <path>` 或 `sisyphus dev --record <path>` 产生）；
#     无记录时各比率为 null（不伪造 0）
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/quality-metrics-collect.sh [records_dir]

Collects single-node quality metrics into
docs/benchmarks/quality-metrics/<YYYY-MM-DD>-<commit>.json

Args:
  records_dir  任务记录目录（默认 docs/benchmarks/quality-metrics/records）

Env:
  BRIDGE_HTTP=http://127.0.0.1:15291   # bridge 未运行时采集为 unavailable
  SISYPHUS_BIN=./target/debug/sisyphus  # 用于聚合任务记录的 CLI
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

RECORDS_DIR="${1:-docs/benchmarks/quality-metrics/records}"
BRIDGE_HTTP="${BRIDGE_HTTP:-http://127.0.0.1:15291}"
SISYPHUS_BIN="${SISYPHUS_BIN:-./target/debug/sisyphus}"
COMMIT="$(git rev-parse --short HEAD)"
STAMP="$(date -u +%Y-%m-%d)"
OUT_DIR="docs/benchmarks/quality-metrics"
OUT="${OUT_DIR}/${STAMP}-${COMMIT}.json"

bridge_up="false"
token_budget_usage="null"
token_budget_capped="null"
token_budget_exhausted="null"
http_requests_total="null"
METRICS_JSON=""
if curl -sf --max-time 2 "${BRIDGE_HTTP}/health" >/dev/null 2>&1; then
  bridge_up="true"
  # 端点是 /api/metrics（不是 /metrics）；失败时不能中断脚本（set -e）
  METRICS_JSON="$(curl -sf --max-time 5 "${BRIDGE_HTTP}/api/metrics" || true)"
  if [ -n "$METRICS_JSON" ]; then
    token_budget_usage="$(printf '%s\n' "$METRICS_JSON" | awk '/^sisyphus_token_budget_usage /{print $NF; exit}')"
    token_budget_capped="$(printf '%s\n' "$METRICS_JSON" | awk '/^sisyphus_token_budget_capped /{print $NF; exit}')"
    token_budget_exhausted="$(printf '%s\n' "$METRICS_JSON" | awk '/^sisyphus_token_budget_exhausted /{print $NF; exit}')"
    http_requests_total="$(printf '%s\n' "$METRICS_JSON" | awk '/^sisyphus_http_requests_total /{print $NF; exit}')"
    token_budget_usage="${token_budget_usage:-null}"
    token_budget_capped="${token_budget_capped:-null}"
    token_budget_exhausted="${token_budget_exhausted:-null}"
    http_requests_total="${http_requests_total:-null}"
  fi
fi

# 任务级指标：调用 CLI 聚合（未构建 CLI 或无记录时回退为 null）
TASK_METRICS_JSON="null"
if [ -x "$SISYPHUS_BIN" ]; then
  if TASK_METRICS_JSON="$("$SISYPHUS_BIN" metrics quality --records "$RECORDS_DIR" --json 2>/dev/null)"; then
    :
  else
    TASK_METRICS_JSON="null"
  fi
else
  echo "note: ${SISYPHUS_BIN} 不可用，任务级指标记为 null" >&2
fi

python3 - "$BRIDGE_HTTP" "$COMMIT" "$STAMP" "$bridge_up" "$token_budget_usage" \
  "$token_budget_capped" "$token_budget_exhausted" "$http_requests_total" \
  "$TASK_METRICS_JSON" "$RECORDS_DIR" <<'PY'
import datetime
import json
import os
import sys

(
    bridge_http,
    commit,
    stamp,
    bridge_up,
    token_budget_usage,
    token_budget_capped,
    token_budget_exhausted,
    http_requests_total,
    task_metrics_json,
    records_dir,
) = sys.argv[1:11]

def as_int(value):
    if value in (None, "", "null"):
        return None
    try:
        return int(float(value))
    except (TypeError, ValueError):
        return None


try:
    task_metrics = json.loads(task_metrics_json)
except (TypeError, ValueError):
    task_metrics = None

metrics = {
    "collected_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "commit": commit,
    "bridge_http": bridge_http,
    "bridge_metrics_available": bridge_up == "true",
    # 指标名取自 bridge /api/metrics 的真实契约（此前脚本里的
    # sisyphus_llm_requests_total / sisyphus_llm_tokens_total 并不存在，
    # 会导致 bridge 在线时仍恒为 null）。
    "token_budget_usage": as_int(token_budget_usage),
    "token_budget_capped": as_int(token_budget_capped),
    "token_budget_exhausted": as_int(token_budget_exhausted),
    "http_requests_total": as_int(http_requests_total),
    "records_dir": records_dir,
}
# 任务级指标：无记录时保持 null（不伪造 0 值）
if task_metrics:
    metrics["task_total"] = task_metrics.get("total")
    metrics["task_passed"] = task_metrics.get("passed")
    metrics["task_failed"] = task_metrics.get("failed")
    metrics["task_blocked"] = task_metrics.get("blocked")
    metrics["task_success_rate"] = task_metrics.get("success_rate")
    metrics["task_fix_rate"] = task_metrics.get("fix_rate")
    metrics["task_regression_rate"] = task_metrics.get("regression_rate")
    metrics["task_cost_duration_ms_per_task"] = task_metrics.get("mean_duration_ms")
    # token 成本维度：只有 provider 真的上报 usage 的记录才参与（无样本为 null）
    metrics["task_cost_tokens_per_task"] = task_metrics.get("mean_total_tokens")
    metrics["task_cost_prompt_tokens_per_task"] = task_metrics.get("mean_prompt_tokens")
    metrics["task_cost_completion_tokens_per_task"] = task_metrics.get("mean_completion_tokens")
else:
    metrics["task_total"] = None
    metrics["task_passed"] = None
    metrics["task_failed"] = None
    metrics["task_blocked"] = None
    metrics["task_success_rate"] = None
    metrics["task_fix_rate"] = None
    metrics["task_regression_rate"] = None
    metrics["task_cost_duration_ms_per_task"] = None
    metrics["task_cost_tokens_per_task"] = None
    metrics["task_cost_prompt_tokens_per_task"] = None
    metrics["task_cost_completion_tokens_per_task"] = None

out_dir = "docs/benchmarks/quality-metrics"
os.makedirs(out_dir, exist_ok=True)
out_path = f"{out_dir}/{stamp}-{commit}.json"
with open(out_path, "w") as f:
    json.dump(metrics, f, indent=2)
    f.write("\n")
print(json.dumps(metrics, indent=2))
PY

printf 'WROTE %s\n' "$OUT"
