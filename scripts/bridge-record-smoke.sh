#!/usr/bin/env bash
# Bridge TaskRecord 落盘冒烟（CI 可复现，无外部依赖）。
#
# 原理：启动 debug bridge，LLM 指向不可达端点（127.0.0.1:1），POST
# /api/execute/goal。目标文本含 "test"，确定性合成器给出单节点
# builtin:bash_exec（无需 LLM）；Swarm 审计因 LLM 不可达走降级链
# （提案不可解析→维持当前配置；意见不可解析→记不批准；迭代上限强制收尾），
# 逻辑仿真与节点执行均不依赖 LLM。因此本场景**确定性产出 status=pass**
# 的 TaskRecord，断言同时覆盖：记录落盘、字段契约、降级链未回归、
# `sisyphus metrics quality` 聚合（含 scenario_breakdown 分组）。
# 不依赖 ollama/云端凭据/外网。
#
# 用法：
#   scripts/bridge-record-smoke.sh            # 自动构建 debug 二进制
#   BRIDGE_BIN=... SISYPHUS_BIN=... scripts/bridge-record-smoke.sh
#
# 约定（与仓库 CI 策略一致）：本脚本串行启动单个 bridge 进程，不接入
# `cargo test --workspace`（bridge 测试因 OOM 约束默认排除）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BRIDGE_BIN="${BRIDGE_BIN:-$ROOT/target/debug/sisyphus-bridge}"
SISYPHUS_BIN="${SISYPHUS_BIN:-$ROOT/target/debug/sisyphus}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/bridge-record-smoke.XXXXXX")"
PORT="${PORT:-8199}"
GRPC_ADDR="${GRPC_ADDR:-http://127.0.0.1:51951}"

cleanup() {
  if [ -n "${BRIDGE_PID:-}" ]; then
    kill "$BRIDGE_PID" 2>/dev/null || true
    wait "$BRIDGE_PID" 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "[smoke] building debug binaries (sisyphus-cli + sisyphus-bridge)..."
cargo build -p sisyphus-cli -p sisyphus-bridge

cat > "$WORK/config.toml" <<'EOF'
[llm]
api_key = "smoke"
endpoint = "http://127.0.0.1:1/v1/chat/completions"
model = "smoke-unavailable"
request_timeout_secs = 2
connect_timeout_secs = 1
fast_fail_cooling_secs = 0

[llm.embedding]
provider = "dummy"
allow_dummy_embeddings = true
EOF

echo "[smoke] starting bridge on 127.0.0.1:$PORT ..."
SISYPHUS_CONFIG="$WORK/config.toml" \
SISYPHUS_HTTP_ADDRESS="http://127.0.0.1:$PORT" \
SISYPHUS_BRIDGE_ADDRESS="$GRPC_ADDR" \
SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR="$WORK/records" \
  "$BRIDGE_BIN" > "$WORK/bridge.log" 2>&1 &
BRIDGE_PID=$!

# 等待 HTTP 端口就绪（任何响应即可，包括 404）。
for _ in $(seq 1 60); do
  if curl -s -m 1 -o /dev/null "http://127.0.0.1:$PORT/"; then
    break
  fi
  if ! kill -0 "$BRIDGE_PID" 2>/dev/null; then
    echo "[smoke] bridge 进程提前退出，日志：" >&2
    tail -20 "$WORK/bridge.log" >&2
    exit 1
  fi
  sleep 1
done

echo "[smoke] POST /api/execute/goal（确定性成功路径，LLM 不可达走降级链）..."
HTTP_CODE="$(curl -s -o "$WORK/response.json" -w '%{http_code}' -m 60 \
  -X POST "http://127.0.0.1:$PORT/api/execute/goal" \
  -H 'content-type: application/json' \
  -d '{"goal":"bridge record smoke test","sop_id":"bridge-record-smoke"}' || true)"
echo "[smoke] http=$HTTP_CODE"
[ "$HTTP_CODE" = "200" ] || { echo "[smoke] FAIL: 预期 HTTP 200" >&2; cat "$WORK/response.json" >&2; exit 1; }

# 给落盘一个排空窗口（记录在 handler 返回前写入，此处仅保守等待）。
sleep 1

echo "[smoke] asserting TaskRecord ..."
RECORDS="$WORK/records"
find "$RECORDS" -name '*.json' | grep -q . || {
  echo "[smoke] FAIL: 无记录落盘" >&2
  tail -30 "$WORK/bridge.log" >&2
  exit 1
}
python3 - "$RECORDS" <<'PY'
import json, sys, glob, pathlib
records = [json.load(open(p)) for p in glob.glob(str(pathlib.Path(sys.argv[1]) / "*.json"))]
assert len(records) == 1, f"expected exactly 1 record, got {len(records)}"
r = records[0]
assert r["scenario"] == "bridge-goal-execute", r
assert r["status"] == "pass", r  # 降级链保证确定性成功（见头部说明）
assert r["task_id"].startswith("bridge-goal-bridge-record-smoke-"), r
assert r["initial_verify_passed"] is True and r["final_verify_passed"] is True, r
assert r["fix_attempts"] == 0 and r["regressed"] is False, r
assert isinstance(r["duration_ms"], int) and r["duration_ms"] > 0, r
assert r["engine_commit"], r  # 版本归因必须存在（本仓库内运行）
# LLM 全不可达：无 provider usage 上报，必须如实为 null（未知 ≠ 0）
assert r["prompt_tokens"] is None and r["completion_tokens"] is None, r
print(f"[smoke] record OK: task_id={r['task_id']} duration_ms={r['duration_ms']} engine_commit={r['engine_commit']}")
PY

echo "[smoke] asserting aggregation ..."
"$SISYPHUS_BIN" metrics quality --records "$RECORDS" --json > "$WORK/aggregate.json"
python3 - "$WORK/aggregate.json" <<'PY'
import json, sys
agg = json.load(open(sys.argv[1]))
assert agg["total"] == 1 and agg["passed"] == 1 and agg["failed"] == 0, agg
assert agg["engine_commits"], agg
# scenario 分组维度（2026-11-14 新增）必须存在且含 bridge-goal-execute
bd = agg.get("scenario_breakdown") or {}
assert "bridge-goal-execute" in bd, agg
sc = bd["bridge-goal-execute"]
assert (sc["total"], sc["passed"]) == (1, 1) and sc["success_rate"] == 1.0, sc
print(f"[smoke] aggregate OK: total={agg['total']} scenario_breakdown={bd}")
PY

echo "[smoke] PASS"
