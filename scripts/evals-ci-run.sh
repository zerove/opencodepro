#!/usr/bin/env bash
# T4.3：evals 子集入 CI —— 选取稳定（criteria 全部为 keyword/regex，无 LLM judge）的 case，
# 经 eval-runner（MockLlmClient，无外部 LLM）运行并记录运行时长与结果。
# 用法: ./scripts/evals-ci-run.sh [cases]      # cases 形如 metis/metis-001,momus/momus-plan-002
#      ./scripts/evals-ci-run.sh               # 默认取稳定 case 全集
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/evals-ci-run.sh [cases]

Options:
  cases  逗号分隔的 case 列表（`agent/id`，如 `metis/metis-001,momus/momus-plan-002`）。
          省略时按「稳定 case」（criteria 全部为 keyword/regex）自动选择。

Writes: docs/benchmarks/evals-ci/<YYYY-MM-DD>-<commit>.json
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

CASES="${1:-}"
if [ -z "$CASES" ]; then
  CASES="$(python3 - <<'PY'
import json, glob, os

agents = ["metis", "momus", "librarian", "scribe", "security"]
cases = []
for agent in agents:
    p = f"evals/{agent}/cases.json"
    if not os.path.exists(p):
        continue
    for c in json.load(open(p)):
        if all(x.get("check_type") in ("keyword", "regex") for x in c.get("criteria", [])):
            cases.append(f"{agent}/{c['id']}")
print(",".join(cases))
PY
)"
fi

COMMIT="$(git rev-parse --short HEAD)"
STAMP="$(date -u +%Y-%m-%d)"
OUT_DIR="docs/benchmarks/evals-ci"
OUT="${OUT_DIR}/${STAMP}-${COMMIT}.json"
RAW="$(mktemp /tmp/opencode/evals-ci-stdout.XXXXXX)"
START_MS="$(date +%s%3N)"
cargo run -q -p eval-runner --bin eval-runner -- --cases "$CASES" > "$RAW"
END_MS="$(date +%s%3N)"
DUR_MS=$((END_MS - START_MS))

python3 - "$COMMIT" "$STAMP" "$CASES" "$DUR_MS" "$RAW" <<'PY'
import datetime
import json
import os
import re
import sys

commit, stamp, cases, dur_ms, raw_path = sys.argv[1:7]
raw = open(raw_path, encoding="utf-8", errors="replace").read()
m = re.search(r"Final Score:\s*([\d.]+)\s*/\s*([\d.]+)\s*\((\d+)/(\d+)\)", raw)
if not m:
    sys.stderr.write("Final Score line not found in eval output\n")
    sys.exit(1)
final_score = float(m.group(1))
final_denominator = float(m.group(2))
passed = int(m.group(3))
total = int(m.group(4))

result = {
    "collected_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "commit": commit,
    "cases": [c for c in cases.split(",") if c],
    "final_score": final_score,
    "final_denominator": final_denominator,
    "passed_count": passed,
    "total_count": total,
    "duration_ms": int(dur_ms),
}
out_dir = "docs/benchmarks/evals-ci"
os.makedirs(out_dir, exist_ok=True)
out_path = f"{out_dir}/{stamp}-{commit}.json"
with open(out_path, "w") as f:
    json.dump(result, f, indent=2)
    f.write("\n")
print(json.dumps(result, indent=2))
PY

rm -f "$RAW"
printf 'WROTE %s\n' "$OUT"
