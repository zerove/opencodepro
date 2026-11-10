#!/usr/bin/env bash
# 创建单机自动化验收证据目录与模板（L0-L4）
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/single-node-acceptance-evidence.sh [tag] [--base DIR]

Creates docs/benchmarks/acceptance/<YYYY-MM-DD>-<tag>/ with evidence templates.
  tag       short label (default: manual)
  --base    override base dir (default: docs/benchmarks/acceptance)

Examples:
  ./scripts/single-node-acceptance-evidence.sh nightly
  ./scripts/single-node-acceptance-evidence.sh l2-check --base /tmp/acceptance
EOF
}

BASE_ROOT="docs/benchmarks/acceptance"
TAG="manual"
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --base) BASE_ROOT="$2"; shift 2 ;;
    *) TAG="$1"; shift ;;
  esac
done

DATE="$(date +%Y-%m-%d)"
DIR="${BASE_ROOT}/${DATE}-${TAG}"
mkdir -p "$DIR"

write_if_missing() {
  local path="$1"
  if [ -f "$path" ]; then
    echo "[skip] exists: $path"
  else
    cat > "$path"
    echo "[new]  $path"
  fi
}

write_if_missing "$DIR/meta.md" <<EOF
# 验收元信息

- date: ${DATE}
- tag: ${TAG}
- commit: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)
- branch: $(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)
- workspace_clean: $(test -z "$(git status --porcelain 2>/dev/null || true)" && echo yes || echo no)
- operator: 
- environment: 
- rustc/cargo: 
- binaries (sisyphus/sisyphus-bridge): 
- config (bridge/http/cluster/llm, 脱敏): 

## 备注

EOF

write_if_missing "$DIR/l0-static.log" <<'EOF'
# L0 静态/契约门禁
# 记录命令、退出码与关键输出摘要
EOF

write_if_missing "$DIR/l1-runtime.log" <<'EOF'
# L1 单机运行时与可观测
# health/readiness/doctor/api/sops/api/cluster/status 等
EOF

write_if_missing "$DIR/l2-sop.log" <<'EOF'
# L2 确定性执行（sop run / 模板 / demo-flow）
EOF

write_if_missing "$DIR/l3-goal.log" <<'EOF'
# L3 目标驱动执行（execute/goal）
# 记录请求、响应(error/hint/report)、trace/事件；缺依赖时标记 BLOCKED
EOF

write_if_missing "$DIR/l4-perf.md" <<'EOF'
# L4 性能与稳定性

| 场景 | n | min | avg | max | 条件/备注 |
|---|---:|---:|---:|---:|---|
EOF

write_if_missing "$DIR/summary.md" <<'EOF'
# 验收摘要

- commit: 
- date: 
- scope: 

| 层级 | 结果(PASS/PASS_WITH_LIMITATIONS/FAIL/BLOCKED) | 证据 | 备注 |
|---|---|---|---|
| L0 |  | l0-static.log |  |
| L1 |  | l1-runtime.log |  |
| L2 |  | l2-sop.log |  |
| L3 |  | l3-goal.log |  |
| L4 |  | l4-perf.md |  |

## 阻塞项

## 风险与差距

## 后续动作
EOF

echo "evidence_dir=$DIR"
