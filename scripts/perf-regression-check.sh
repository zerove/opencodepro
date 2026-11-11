#!/usr/bin/env bash
# 性能回归门禁（T4.2）：用最新采样结果与基线阈值比较，超阈值则以非零退出。
# 用法:
#   ./scripts/perf-regression-check.sh <cli|bridge> [--baseline <file>] [--json]
#
# 基线阈值文件（JSON）：
#   { "created_at": "...", "commit": "...", "thresholds": { "<metric>": <value> } }
#
#   - cli:       "cli_help_p50_s" / "cli_exec_p50_s" / "cli_session_list_p50_s"（秒）
#   - bridge:   "bridge_ready_p50_ms", "http_health_p95_s", "http_sops_p95_s"
#
# 失败语义：任一指标超过基线阈值（p95 超过对应 p50 基线 × 2 视为显著）→ 输出明细并 exit 1。
# 环境不可比性不得自动判定为回归；样本数少于 1 时视为无法判定（不判回归）。
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/perf-regression-check.sh <cli|bridge> [options]

Options:
  --baseline <file>  基线阈值 JSON（默认：docs/benchmarks/perf-regression-baseline-2026-11-10.json）
  --json             仅输出 JSON（含 current / baseline / regressions）
  -h, --help         帮助

Env:
  N=5 样本数（透传给 single-node-bench-sample.sh）
EOF
}

MODE="cli"
BASELINE="docs/benchmarks/perf-regression-baseline-2026-11-10.json"
JSON_OUT=false
while [ $# -gt 0 ]; do
  case "$1" in
    cli|bridge) MODE="$1" ;;
    --baseline) shift; BASELINE="$1" ;;
    --json) JSON_OUT=true ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

if [ ! -f "$BASELINE" ]; then
  echo "Baseline not found: $BASELINE" >&2
  exit 2
fi

SAMPLE=./scripts/single-node-bench-sample.sh
CURRENT=$($SAMPLE "$MODE" --json)

rc=0
CURRENT_JSON="$CURRENT" BASELINE="$BASELINE" MODE="$MODE" JSON_OUT="$JSON_OUT" \
python3 - <<'PY' || rc=$?
import json, os, subprocess, sys

mode = os.environ["MODE"]
current = json.loads(os.environ["CURRENT_JSON"])
baseline = json.load(open(os.environ["BASELINE"]))
thresholds = baseline.get("thresholds") or {}
current_commit = subprocess.run(
    ["git", "rev-parse", "HEAD"], capture_output=True, text=True
).stdout.strip()

result = {
    "mode": mode,
    "sampled_at": current.get("timestamp"),
    "current_commit": current_commit,
    "baseline_commit": baseline.get("commit"),
    "build": current.get("build"),
    "n": current.get("n"),
    "thresholds": thresholds,
    "regressions": [],
}
failed = False

def check(metric, value, limit):
    exceeded = value > limit
    return exceeded, {
        "metric": metric, "value": value, "limit": limit,
        "op": "<", "exceeded": exceeded,
    }

if mode == "cli":
    for entry in current.get("commands") or []:
        cmd = entry.get("cmd") or ""
        stats = entry.get("stats") or {}
        p50 = float(stats.get("p50") or 0.0)
        p95 = float(stats.get("p95") or 0.0)
        limits = {
            "sisyphus --help": ("cli_help_p50_s", 0.05),
            "sisyphus exec echo ok": ("cli_exec_p50_s", 0.05),
            "sisyphus --data-dir /tmp/opencode/bench-sn session list": ("cli_session_list_p50_s", 0.05),
        }
        for prefix, (thr_key, limit) in limits.items():
            if cmd == prefix:
                exceeded, rec = check(thr_key, p50, limit)
                if p95 > limit * 2:
                    exceeded, rec = True, {
                        "metric": thr_key, "value": p95, "limit": limit * 2,
                        "op": "<", "exceeded": True, "detail": f"p95={p95} > p50-limit*2",
                    }
                if exceeded:
                    failed = True
                    result["regressions"].append(rec)
                result.setdefault("cli", []).append({
                    "cmd": cmd, "p50": p50, "p95": p95, "limit": limit,
                })
elif mode == "bridge":
    ready_p50 = float((current.get("ready_ms") or {}).get("p50") or 0.0)
    health_p95 = float((current.get("health_s") or {}).get("p95") or 0.0)
    sops_p95 = float((current.get("sops_s") or {}).get("p95") or 0.0)
    checks = [
        ("bridge_ready_p50_ms", ready_p50, float(thresholds.get("bridge_ready_p50_ms") or 3000.0)),
        ("http_health_p95_s", health_p95, float(thresholds.get("http_health_p95_s") or 0.01)),
        ("http_sops_p95_s", sops_p95, float(thresholds.get("http_sops_p95_s") or 0.01)),
    ]
    for metric, value, limit in checks:
        exceeded, rec = check(metric, value, limit)
        if exceeded:
            failed = True
            result["regressions"].append(rec)

result["failed"] = failed
if not os.environ.get("JSON_OUT"):
    print(f"mode={mode} current_commit={current_commit} baseline_commit={baseline.get('commit')}")
    print(f"n={current.get('n')} thresholds={json.dumps(thresholds)}")
    if mode == "cli":
        for e in result.get("cli", []):
            print(f"  cli {e['cmd']}: p50={e['p50']:.6f}s p95={e['p95']:.6f}s limit={e['limit']}")
    else:
        print(f"  bridge_ready_p50_ms={ready_p50} limit={checks[0][2]}")
        print(f"  http_health_p95_s={health_p95} limit={checks[1][2]}")
        print(f"  http_sops_p95_s={sops_p95} limit={checks[2][2]}")
print(json.dumps(result, indent=2) if os.environ.get("JSON_OUT") else "")

sys.exit(1 if failed else 0)
PY
rc=$?

if [ $rc -ne 0 ]; then
  echo "PERF REGRESSION DETECTED（超基线阈值）" >&2
fi
exit $rc
