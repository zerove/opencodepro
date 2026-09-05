#!/usr/bin/env bash
# Bench dashboard 生成脚本
#
# 流程：
#   1. 跑 criterion bench（生成 target/criterion/<group>/<name>/）
#   2. 生成 index.html 列出所有 bench
#   3. 复制 HTML 报告到 docs/benchmarks/dashboard/
#
# 用法：
#   ./scripts/bench-dashboard.sh                       # 跑全部
#   ./scripts/bench-dashboard.sh --bench core_benches  # 仅跑 core_benches
#   ./scripts/bench-dashboard.sh --output-dir docs/benchmarks/dashboard
#
# 退出码：0 = 成功，非零 = 失败

set -euo pipefail

# ── Configuration ──────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BENCH_FILTER="${1:-}"
OUTPUT_DIR="${OUTPUT_DIR:-$PROJECT_ROOT/docs/benchmarks/dashboard}"
TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CRITERION_DIR="$PROJECT_ROOT/target/criterion"

# ── Helpers ────────────────────────────────────────────────────────────
log() { printf "[bench-dashboard] %s\n" "$*" >&2; }
err() { printf "[bench-dashboard] ERROR: %s\n" "$*" >&2; }

# ── Pre-flight checks ──────────────────────────────────────────────────
if ! command -v cargo >/dev/null 2>&1; then
    err "cargo not found in PATH"
    exit 1
fi

if [[ ! -d "$PROJECT_ROOT/crates/sisyphus-core/benches" ]]; then
    err "sisyphus-core benches not found at $PROJECT_ROOT/crates/sisyphus-core/benches"
    exit 1
fi

# ── Run benchmarks ─────────────────────────────────────────────────────
log "running benchmarks (this may take several minutes)..."

BENCH_CMD="cargo bench -p sisyphus-core --features sandbox --bench core_benches"
if [[ -n "$BENCH_FILTER" ]]; then
    BENCH_CMD="$BENCH_CMD -- --bench $BENCH_FILTER"
fi

log "executing: $BENCH_CMD"
if ! $BENCH_CMD > /tmp/bench-output.log 2>&1; then
    err "benchmark failed; see /tmp/bench-output.log"
    tail -30 /tmp/bench-output.log >&2
    exit 1
fi

if [[ ! -d "$CRITERION_DIR" ]]; then
    err "criterion output dir not found: $CRITERION_DIR"
    exit 1
fi

# ── Generate dashboard HTML ───────────────────────────────────────────
log "generating dashboard index at $OUTPUT_DIR/index.html"
mkdir -p "$OUTPUT_DIR"

# Collect bench list
BENCH_LIST=$(find "$CRITERION_DIR" -mindepth 2 -maxdepth 2 -type d | sort)

if [[ -z "$BENCH_LIST" ]]; then
    err "no bench directories found in $CRITERION_DIR"
    exit 1
fi

# Write index.html with embedded bench list
{
cat <<EOF
<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<title>Sisyphus Benchmark Dashboard — ${TIMESTAMP}</title>
<style>
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
       max-width: 960px; margin: 2rem auto; padding: 0 1rem; color: #333; }
h1 { border-bottom: 2px solid #eee; padding-bottom: 0.5rem; }
.meta { color: #666; font-size: 0.9em; margin-bottom: 2rem; }
table { width: 100%; border-collapse: collapse; }
th, td { padding: 0.6em 1em; text-align: left; border-bottom: 1px solid #eee; }
th { background: #f8f8f8; font-weight: 600; }
a { color: #0366d6; text-decoration: none; }
a:hover { text-decoration: underline; }
code { background: #f6f8fa; padding: 0.1em 0.4em; border-radius: 3px;
       font-family: ui-monospace, "Cascadia Code", Menlo, monospace; font-size: 0.9em; }
.empty { color: #999; font-style: italic; }
</style>
</head>
<body>
<h1>Sisyphus Benchmark Dashboard</h1>
<p class="meta">
  Generated: <code>${TIMESTAMP}</code><br>
  Criterion version: see Cargo.lock<br>
  Branch: <code>$(cd "$PROJECT_ROOT" && git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)</code><br>
  Commit: <code>$(cd "$PROJECT_ROOT" && git rev-parse --short HEAD 2>/dev/null || echo unknown)</code>
</p>

<h2>Benchmarks (${BENCH_COUNT})</h2>
<table>
<thead>
<tr>
<th>Bench (group / function)</th>
<th>HTML Report</th>
<th>Estimated mean (latest run)</th>
</tr>
</thead>
<tbody>
EOF

# Iterate benches and emit table rows
BENCH_COUNT=0
while IFS= read -r bench_dir; do
    REL="${bench_dir#$CRITERION_DIR/}"
    GROUP="${REL%%/*}"
    NAME="${REL##*/}"
    BENCH_COUNT=$((BENCH_COUNT + 1))

    # criterion writes estimates.json with mean (in ns)
    ESTIMATE="—"
    if [[ -f "$bench_dir/estimate.json" ]]; then
        POINT_EST=$(grep -o '"point_estimate":[[:space:]]*[0-9.eE+-]*' "$bench_dir/estimate.json" \
                    | head -1 | cut -d: -f2 | tr -d ' ' || echo "")
        if [[ -n "$POINT_EST" ]]; then
            # Format: ns → µs if ≥ 1000ns
            if command -v python3 >/dev/null 2>&1; then
                ESTIMATE=$(python3 -c "
import sys
ns = float('$POINT_EST')
if ns >= 1_000_000:
    print(f'{ns/1_000_000:.2f} ms')
elif ns >= 1_000:
    print(f'{ns/1_000:.2f} µs')
else:
    print(f'{ns:.1f} ns')
")
            else
                ESTIMATE="${POINT_EST} ns"
            fi
        fi
    fi

    cat <<ROW
<tr>
<td><code>${GROUP} / ${NAME}</code></td>
<td><a href="${GROUP}/${NAME}/report/index.html">view report →</a></td>
<td>${ESTIMATE}</td>
</tr>
ROW
done <<< "$BENCH_LIST"

cat <<'EOF'
</tbody>
</table>

<p class="empty">
提示：完整 criterion 报告含每次采样的分布、回归检测与图表，
点击 "view report" 查看。
</p>
</body>
</html>
EOF
} > "$OUTPUT_DIR/index.html"

log "dashboard written: $OUTPUT_DIR/index.html"
log "open in browser: file://$OUTPUT_DIR/index.html"
log "bench_count: $BENCH_COUNT"