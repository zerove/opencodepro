#!/usr/bin/env bash
# T3.3：TUI 首帧 PTY 测量（进程启动 → 首帧可见的端到端耗时）。
#
# 用法: ./scripts/measure-tui-first-frame.sh [bin] [samples]
#   bin      TUI 二进制路径（默认 ./target/debug/sisyphus）
#   samples  采样次数（默认 3）
#
# 输出: docs/benchmarks/tui-first-frame/<YYYY-MM-DD>-<commit>.json
#       { first_frame_ms: {samples, p50, min, max, avg}, ... }
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/measure-tui-first-frame.sh [bin] [samples]

Measures TUI time-to-first-frame in a pseudo-terminal (headless).
Default bin: ./target/debug/sisyphus   Default samples: 3
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

BIN="${1:-./target/debug/sisyphus}"
SAMPLES="${2:-3}"
DATA_DIR="${DATA_DIR:-/tmp/opencode/tui-first-frame-data}"

if [ ! -x "$BIN" ]; then
  echo "TUI binary not found or not executable: $BIN" >&2
  echo "Build it first: cargo build -p sisyphus-cli" >&2
  exit 2
fi

COMMIT="$(git rev-parse --short HEAD)"
STAMP="$(date -u +%Y-%m-%d)"
OUT_DIR="docs/benchmarks/tui-first-frame"
OUT="${OUT_DIR}/${STAMP}-${COMMIT}.json"

mkdir -p "$OUT_DIR" "$DATA_DIR"

python3 - "$BIN" "$SAMPLES" "$DATA_DIR" "$OUT" "$COMMIT" "$STAMP" <<'PY'
import datetime
import fcntl
import json
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import termios
import time

binary, samples_n, data_dir, out_path, commit, stamp = sys.argv[1:7]
samples_n = int(samples_n)

MARKER = b"\xe2\x94"  # UTF-8 制表符（┌ ─ │），首页面板绘制即视为首帧可见


def measure_once():
    """在伪终端中启动 TUI，测量首帧可见耗时（毫秒）。"""
    pid, fd = pty.fork()
    if pid == 0:  # 子进程
        os.environ["TERM"] = "xterm-256color"
        try:
            os.execv(
                binary,
                [binary, "--data-dir", data_dir, "--bridge", "127.0.0.1:50051"],
            )
        except Exception:
            os._exit(127)

    try:
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
    except OSError:
        pass

    t0 = time.perf_counter()
    first_frame_ms = None
    buf = b""
    deadline = t0 + 30.0
    while time.perf_counter() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.2)
        if not ready:
            continue
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            break
        if not chunk:
            break
        buf += chunk
        if MARKER in buf:
            first_frame_ms = (time.perf_counter() - t0) * 1000.0
            break

    # 退出 TUI（发送 'q'；随后强制回收）
    try:
        os.write(fd, b"q")
    except OSError:
        pass
    time.sleep(0.3)
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        os.waitpid(pid, os.WNOHANG)
    except ChildProcessError:
        pass
    try:
        os.close(fd)
    except OSError:
        pass
    return first_frame_ms


samples = []
for _ in range(samples_n):
    samples.append(measure_once())

ok = [s for s in samples if s is not None]
stats = {
    "samples": [None if s is None else round(s, 1) for s in samples],
    "measured": len(ok),
    "requested": samples_n,
}
if ok:
    ordered = sorted(ok)
    stats["p50"] = round(ordered[len(ordered) // 2], 1)
    stats["min"] = round(ordered[0], 1)
    stats["max"] = round(ordered[-1], 1)
    stats["avg"] = round(sum(ordered) / len(ordered), 1)

result = {
    "collected_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "commit": commit,
    "binary": binary,
    "metric": "tui_first_frame_ms",
    "first_frame_ms": stats,
    "status": "ok" if len(ok) == samples_n else "partial",
}

with open(out_path, "w") as f:
    json.dump(result, f, indent=2)
    f.write("\n")
print(json.dumps(result, indent=2))
PY

printf 'WROTE %s\n' "$OUT"
