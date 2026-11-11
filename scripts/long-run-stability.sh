#!/usr/bin/env bash
# T4.5：长窗口稳定性监测（内存 / 句柄 / 线程 / DB 增长 + 周期性只读负载）。
#
# 用法: ./scripts/long-run-stability.sh [minutes]
#   minutes  监测窗口分钟数（默认 10；CI/本地可调）
#
# 产出: docs/benchmarks/long-run/<YYYY-MM-DD>-<commit>/
#   samples.tsv   原始采样（时间戳 / RSS / FD / 线程 / 探活结果）
#   summary.json  统计摘要（首末值、斜率、是否泄漏迹象）
#   summary.md    结论说明（明确实际监测窗口，不外推未测时长）
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/long-run-stability.sh [minutes]

Monitors a running bridge for resource leaks over a fixed window.
Default: 10 minutes. Writes docs/benchmarks/long-run/<date>-<commit>/.
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

MINUTES="${1:-10}"
INTERVAL="${INTERVAL:-5}"
BIN="${BIN:-./target/debug/sisyphus-bridge}"
HTTP="${HTTP:-http://127.0.0.1:15291}"
GRPC_PORT="${GRPC_PORT:-15261}"
DATA_DIR="${DATA_DIR:-$TMP_ROOT/long-run-data}"
# CI runner 上 /tmp/opencode 不存在，配置写入会失败 → 先确保目录存在
TMP_ROOT="${TMPDIR:-/tmp}/opencode"
mkdir -p "$TMP_ROOT"
CFG="${CFG:-$TMP_ROOT/long-run-bridge.toml}"
COMMIT="$(git rev-parse --short HEAD)"
STAMP="$(date -u +%Y-%m-%d)"
REPO_ROOT="$(pwd)"
OUT_DIR="${REPO_ROOT}/docs/benchmarks/long-run/${STAMP}-${COMMIT}"
SAMPLES="${OUT_DIR}/samples.tsv"

if [ ! -x "$BIN" ]; then
  echo "bridge binary not found: $BIN" >&2
  echo "build it first: cargo build -p sisyphus-bridge" >&2
  exit 2
fi

# 采样依赖 Linux /proc（RSS / FD / 线程）；其它平台无法得到等价指标，
# 明确失败而不是产出全 0 的假数据。
if [ ! -r "/proc/self/status" ]; then
  echo "/proc is not available on this platform; resource sampling is Linux-only" >&2
  exit 2
fi

# 转绝对路径（脚本后续会 cd 到数据目录）
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"

mkdir -p "$OUT_DIR" "$DATA_DIR"

cleanup() {
  if [ -n "${BPID:-}" ]; then
    kill "$BPID" 2>/dev/null || true
    wait "$BPID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# 端口占用守卫：若目标端口已被其它实例占用，健康探测会打到**别的进程**，
# 测出的「就绪耗时」属于那个进程，本次测量失真（实测踩过：与长窗口监测
# 同端口时测出 45ms 的假数据）。
if curl -sf --max-time 1 "${HTTP}/health" >/dev/null 2>&1; then
  echo "HTTP 端口 ${HTTP} 已被占用：健康探测会打到既有实例，测量会失真" >&2
  echo "请换一个端口（HTTP=<host:port>）后重试" >&2
  exit 2
fi
# 端口占用检测（HTTP + gRPC）：任一端口已被其它实例占用时，
# 本脚本启动的进程会绑定失败，而健康探测可能打到**别的实例** → 测量失真。
port_in_use() {
  python3 - "$1" <<'PY_PORT'
import socket, sys
port = int(sys.argv[1])
sock = socket.socket()
sock.settimeout(0.5)
try:
    sock.connect(("127.0.0.1", port))
    sys.exit(0)
except OSError:
    sys.exit(1)
finally:
    sock.close()
PY_PORT
}

if port_in_use "$GRPC_PORT"; then
  echo "gRPC 端口 ${GRPC_PORT} 已被占用：本次启动会绑定失败，测量无意义" >&2
  echo "请换一个端口（GRPC_PORT=<port>）后重试" >&2
  exit 2
fi

cat > "$CFG" <<TOML
bridge_address = "http://127.0.0.1:${GRPC_PORT}"
http_address = "$HTTP"

[cluster]
enabled = false
TOML

cd "$DATA_DIR" || exit 2
SISYPHUS_CONFIG="$CFG" SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true "$BIN" >"${OUT_DIR}/bridge.log" 2>&1 &
BPID=$!

READY=0
for _ in $(seq 1 60); do
  if curl -sf --max-time 2 "${HTTP}/health" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 0.5
done
if [ "$READY" != "1" ]; then
  echo "bridge did not become ready" >&2
  exit 2
fi

printf 'ts\trss_kb\tfds\tthreads\thealth\tmetrics\n' > "$SAMPLES"
START_EPOCH="$(date +%s)"
DEADLINE=$((START_EPOCH + MINUTES * 60))

while [ "$(date +%s)" -lt "$DEADLINE" ]; do
  HEALTH=0
  METRICS=0
  curl -sf --max-time 3 "${HTTP}/health" >/dev/null 2>&1 && HEALTH=1
  curl -sf --max-time 3 "${HTTP}/api/metrics" >/dev/null 2>&1 && METRICS=1

  RSS_KB="$(awk '/^VmRSS:/ {print $2}' "/proc/${BPID}/status" 2>/dev/null || echo "")"
  FDS="$(ls "/proc/${BPID}/fd" 2>/dev/null | wc -l | tr -d ' ')"
  THREADS="$(awk '/^Threads:/ {print $2}' "/proc/${BPID}/status" 2>/dev/null || echo "")"
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$(date +%s)" "${RSS_KB:-0}" "${FDS:-0}" "${THREADS:-0}" "$HEALTH" "$METRICS" >> "$SAMPLES"

  sleep "$INTERVAL"
done

END_EPOCH="$(date +%s)"

python3 - "$SAMPLES" "${OUT_DIR}/summary.json" "${OUT_DIR}/summary.md" \
  "$COMMIT" "$STAMP" "$((END_EPOCH - START_EPOCH))" "$BIN" <<'PY'
import json
import sys

samples_path, json_path, md_path, commit, stamp, elapsed_s, binary = sys.argv[1:8]

rows = []
with open(samples_path, encoding="utf-8") as f:
    header = f.readline().strip().split("\t")
    for line in f:
        parts = line.strip().split("\t")
        if len(parts) != len(header):
            continue
        row = dict(zip(header, parts))
        try:
            row["rss_kb"] = int(row["rss_kb"])
            row["fds"] = int(row["fds"])
            row["threads"] = int(row["threads"])
            row["health"] = int(row["health"])
            row["metrics"] = int(row["metrics"])
            row["ts"] = int(row["ts"])
        except ValueError:
            continue
        rows.append(row)


def slope(values):
    """最小二乘斜率（每分钟变化量）。"""
    if len(values) < 2:
        return 0.0
    n = len(values)
    xs = list(range(n))
    mean_x = sum(xs) / n
    mean_y = sum(values) / n
    denom = sum((x - mean_x) ** 2 for x in xs)
    if denom == 0:
        return 0.0
    num = sum((x - mean_x) * (y - mean_y) for x, y in zip(xs, values))
    per_sample = num / denom
    return per_sample * (60.0 / 5.0)  # 采样间隔 5s → 每分钟


summary = {
    "collected_at": stamp,
    "commit": commit,
    "binary": binary,
    "elapsed_seconds": int(elapsed_s),
    "sample_count": len(rows),
    "sample_interval_seconds": 5,
    "health_ok_ratio": (sum(r["health"] for r in rows) / len(rows)) if rows else None,
    "metrics_ok_ratio": (sum(r["metrics"] for r in rows) / len(rows)) if rows else None,
    "rss_kb": {
        "first": rows[0]["rss_kb"] if rows else None,
        "last": rows[-1]["rss_kb"] if rows else None,
        "min": min((r["rss_kb"] for r in rows), default=None),
        "max": max((r["rss_kb"] for r in rows), default=None),
        "slope_per_min": round(slope([r["rss_kb"] for r in rows]), 2) if rows else None,
    },
    "fds": {
        "first": rows[0]["fds"] if rows else None,
        "last": rows[-1]["fds"] if rows else None,
        "min": min((r["fds"] for r in rows), default=None),
        "max": max((r["fds"] for r in rows), default=None),
        "slope_per_min": round(slope([r["fds"] for r in rows]), 2) if rows else None,
    },
    "threads": {
        "first": rows[0]["threads"] if rows else None,
        "last": rows[-1]["threads"] if rows else None,
        "min": min((r["threads"] for r in rows), default=None),
        "max": max((r["threads"] for r in rows), default=None),
        "slope_per_min": round(slope([r["threads"] for r in rows]), 2) if rows else None,
    },
}

# 泄漏判定：RSS/FD 线性斜率为正且末值显著高于首值（阈值保守，避免误报）
def leak_verdict(metric):
    if not rows or metric["slope_per_min"] is None:
        return "unknown"
    growth = metric["last"] - metric["first"]
    if metric["slope_per_min"] > 0 and growth > max(10, 0.05 * max(metric["first"], 1)):
        return "possible_leak"
    if metric["slope_per_min"] < 0 or growth < 0:
        return "decreasing"
    return "stable"


summary["verdict"] = {
    "rss": leak_verdict(summary["rss_kb"]),
    "fds": leak_verdict(summary["fds"]),
    "threads": leak_verdict(summary["threads"]),
}

with open(json_path, "w") as f:
    json.dump(summary, f, indent=2)
    f.write("\n")

minutes = int(elapsed_s) / 60.0
md = f"""# T4.5 长窗口稳定性监测结论

- 采集时间（UTC）：{stamp}
- commit：`{commit}`
- 二进制：`{binary}`
- **实际监测窗口：{int(elapsed_s)} 秒（约 {minutes:.1f} 分钟）**，采样间隔 5 秒，共 {len(rows)} 个样本
- 负载：每轮对 `/health` 与 `/api/metrics` 各发起一次只读请求

| 指标 | 首值 | 末值 | 最小 | 最大 | 斜率（每分钟） | 判定 |
|---|---|---|---|---|---|---|
| RSS (KB) | {summary['rss_kb']['first']} | {summary['rss_kb']['last']} | {summary['rss_kb']['min']} | {summary['rss_kb']['max']} | {summary['rss_kb']['slope_per_min']} | {summary['verdict']['rss']} |
| 打开的 FD 数 | {summary['fds']['first']} | {summary['fds']['last']} | {summary['fds']['min']} | {summary['fds']['max']} | {summary['fds']['slope_per_min']} | {summary['verdict']['fds']} |
| 线程数 | {summary['threads']['first']} | {summary['threads']['last']} | {summary['threads']['min']} | {summary['threads']['max']} | {summary['threads']['slope_per_min']} | {summary['verdict']['threads']} |

- 探活成功率：{summary['health_ok_ratio']}；metrics 端点成功率：{summary['metrics_ok_ratio']}

## 结论口径

- 本结论**仅覆盖上述实际监测窗口**（约 {minutes:.1f} 分钟），不外推到"数小时"结论。
- 如需更长窗口，用同一脚本加大分钟参数（如 `./scripts/long-run-stability.sh 240`）重新采集并另存目录。
- 泄漏判定为启发式：斜率为正且末值相对首值增长超过 5%（或 10 个绝对单位）时标记 `possible_leak`。
"""
with open(md_path, "w") as f:
    f.write(md)
print(json.dumps(summary, indent=2))
PY

printf 'WROTE %s\n' "${OUT_DIR}"
