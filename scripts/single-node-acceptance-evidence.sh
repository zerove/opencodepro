#!/usr/bin/env bash
# 创建单机自动化验收证据目录与模板（L0-L4）
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/single-node-acceptance-evidence.sh [tag] [--base DIR] [--run-l0] [--run-l1]

Creates docs/benchmarks/acceptance/<YYYY-MM-DD>-<tag>/ with evidence templates.
  tag       short label (default: manual)
  --base    override base dir (default: docs/benchmarks/acceptance)
  --run-l0  运行 L0 快速检查（模板校验/脚本语法）并写入 l0-static.log
            （设置 L0_FULL_SMOKE=1 追加 contract-smoke --quick，耗时较长）
  --run-l1  真实启动 debug bridge（dummy LLM 配置），跑 health/metrics 契约
            与 readiness-check，写入 l1-runtime.log（端口可用 L1_PORT 覆盖）

Examples:
  ./scripts/single-node-acceptance-evidence.sh nightly
  ./scripts/single-node-acceptance-evidence.sh nightly --run-l0
  ./scripts/single-node-acceptance-evidence.sh l2-check --base /tmp/acceptance
EOF
}

BASE_ROOT="docs/benchmarks/acceptance"
TAG="manual"
RUN_L0=false
RUN_L1=false
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --base) BASE_ROOT="$2"; shift 2 ;;
    --run-l0) RUN_L0=true; shift ;;
    --run-l1) RUN_L1=true; shift ;;
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
#
# 建议命令（按环境调整）：
#   TEST_GOAL=1 GOAL_TEXT="..." BASE=http://127.0.0.1:8080 SOP_ID=verify_sop ./scripts/single-node-demo-flow.sh
#   curl -sS -X POST "$BASE/api/execute/goal" -H 'Content-Type: application/json' \
#     -d '{"goal":"...","sop_id":"verify_sop","project_id":"default"}'
#
# 关注：HTTP 状态、error/hint 字段、report/output、trace/事件可关联性；与 sop run 路径差异。
EOF

write_if_missing "$DIR/l4-perf.md" <<'EOF'
# L4 性能与稳定性

> 采集建议：`./scripts/single-node-bench-sample.sh cli --json` / `bridge --json`；记录 debug/release、冷热、构建与端口配置。
> 不中断即不伪造连续样本；分批采集需标注聚合方式与环境限制。

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

# 通用步骤记录器：命令、退出码与输出尾部追加到当前层级日志。
run_step() {
  local name="$1"
  shift
  local out rc
  out="$(timeout 300 "$@" 2>&1)"
  rc=$?
  {
    echo ""
    echo "### $name"
    echo "cmd: $*"
    echo "exit: $rc"
    echo '```text'
    printf '%s\n' "$out" | tail -40
    echo '```'
  } >> "$LOG"
  if [ "$rc" -ne 0 ]; then
    echo "[WARN] step failed: $name (exit=$rc)" >&2
  fi
}

if [ "$RUN_L0" = true ]; then
  LOG="$DIR/l0-static.log"
  {
    echo ""
    echo "## $(date -u +%Y-%m-%dT%H:%M:%SZ) L0 quick run"
    echo "commit: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
    echo "rustc: $(rustc -V 2>/dev/null || echo unavailable)"
    echo "cargo: $(cargo -V 2>/dev/null || echo unavailable)"
  } >> "$LOG"

  run_step() {
    local name="$1"
    shift
    local out rc
    out="$(timeout 300 "$@" 2>&1)"
    rc=$?
    {
      echo ""
      echo "### $name"
      echo "cmd: $*"
      echo "exit: $rc"
      echo '```text'
      printf '%s\n' "$out" | tail -40
      echo '```'
    } >> "$LOG"
    if [ "$rc" -ne 0 ]; then
      echo "[WARN] L0 step failed: $name (exit=$rc)" >&2
    fi
  }

  set +e
  run_step "validate-sop-templates" ./scripts/validate-sop-templates.sh
  run_step "bash syntax (scripts)" bash -n \
    scripts/single-node-readiness-check.sh \
    scripts/single-node-demo-flow.sh \
    scripts/single-node-apply-sop-template.sh \
    scripts/single-node-bench-sample.sh \
    scripts/single-node-contract-smoke.sh \
    scripts/single-node-acceptance-evidence.sh
  if [ "${L0_FULL_SMOKE:-0}" = "1" ]; then
    run_step "contract-smoke --quick" ./scripts/single-node-contract-smoke.sh --quick
  else
    {
      echo ""
      echo "### contract-smoke --quick"
      echo "skipped (set L0_FULL_SMOKE=1 to enable)"
    } >> "$LOG"
  fi
  set -e
  echo "l0_log=$LOG"
fi

if [ "$RUN_L1" = true ]; then
  LOG="$DIR/l1-runtime.log"
  {
    echo ""
    echo "## $(date -u +%Y-%m-%dT%H:%M:%SZ) L1 runtime run"
    echo "commit: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
    echo "rustc: $(rustc -V 2>/dev/null || echo unavailable)"
  } >> "$LOG"

  L1_WORK="$(mktemp -d /tmp/sn-l1.XXXXXX)"
  L1_PORT="${L1_PORT:-18080}"
  L1_BASE="http://127.0.0.1:$L1_PORT"
  L1_PID=""
  cleanup_l1() {
    if [ -n "$L1_PID" ]; then
      kill "$L1_PID" 2>/dev/null || true
      wait "$L1_PID" 2>/dev/null || true
    fi
    rm -rf "$L1_WORK"
  }
  trap cleanup_l1 EXIT

  # dummy LLM 配置：L1 只验证运行时与可观测面，不依赖真实 provider。
  cat > "$L1_WORK/config.toml" <<'EOF'
[llm]
api_key = "l1-evidence"
endpoint = "http://127.0.0.1:1/v1/chat/completions"
model = "l1-unavailable"
request_timeout_secs = 2
connect_timeout_secs = 1
fast_fail_cooling_secs = 0

[llm.embedding]
provider = "dummy"
allow_dummy_embeddings = true
EOF

  run_step "cargo build (bridge debug)" cargo build -p sisyphus-bridge

  SISYPHUS_CONFIG="$L1_WORK/config.toml" \
  SISYPHUS_HTTP_ADDRESS="$L1_BASE" \
  SISYPHUS_BRIDGE_ADDRESS="http://127.0.0.1:15051" \
  SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR="$L1_WORK/records" \
    ./target/debug/sisyphus-bridge >"$L1_WORK/bridge.log" 2>&1 &
  L1_PID=$!

  L1_UP=0
  for _ in $(seq 1 60); do
    if curl -s -m 1 -o /dev/null "$L1_BASE/"; then L1_UP=1; break; fi
    if ! kill -0 "$L1_PID" 2>/dev/null; then break; fi
    sleep 1
  done

  if [ "$L1_UP" != "1" ]; then
    {
      echo ""
      echo "### bridge startup"
      echo "exit: bridge 未在 60s 内就绪"
      echo '```text'
      tail -30 "$L1_WORK/bridge.log"
      echo '```'
    } >> "$LOG"
    echo "[WARN] L1 step failed: bridge startup" >&2
  else
    run_step "GET /health" curl -fsS -m 5 "$L1_BASE/health"
    run_step "GET /health/ready" curl -fsS -m 5 "$L1_BASE/health/ready"
    run_step "GET /api/sops" curl -fsS -m 5 "$L1_BASE/api/sops"
    run_step "GET /api/cluster/status" curl -fsS -m 5 "$L1_BASE/api/cluster/status"
    # /api/metrics 契约断言：指标名必须真实存在（与 quality-metrics-collect.sh 读取的契约一致）
    if curl -fsS -m 5 "$L1_BASE/api/metrics" > "$L1_WORK/metrics.txt" \
       && grep -q '^sisyphus_http_requests_total' "$L1_WORK/metrics.txt" \
       && grep -q '^sisyphus_token_budget_usage' "$L1_WORK/metrics.txt"; then
      {
        echo ""
        echo "### /api/metrics contract"
        echo "exit: 0"
        echo '```text'
        echo "sisyphus_* 指标行数: $(grep -c '^sisyphus_' "$L1_WORK/metrics.txt")"
        grep '^sisyphus_http_requests_total\|^sisyphus_token_budget_usage' "$L1_WORK/metrics.txt"
        echo '```'
      } >> "$LOG"
    else
      run_step "/api/metrics contract (FAILED)" \
        grep -n 'sisyphus_http_requests_total\|sisyphus_token_budget_usage' "$L1_WORK/metrics.txt"
    fi

    # 官方 readiness 工具（失败不中断 L1，仅记录）
    set +e
    run_step "single-node-readiness-check" \
      env BASE="$L1_BASE" WS=default ./scripts/single-node-readiness-check.sh
    set -e
  fi

  cleanup_l1
  trap - EXIT
  echo "l1_log=$LOG"
fi

echo "evidence_dir=$DIR"
