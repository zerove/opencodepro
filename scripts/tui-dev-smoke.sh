#!/usr/bin/env bash
# T2.1：TUI 对话式执行 MVP 的真实交互冒烟（伪终端驱动）。
#
# 用法: ./scripts/tui-dev-smoke.sh [bin]
#   bin  TUI 二进制（默认 ./target/debug/sisyphus）
#
# 覆盖路径：Ctrl+N 新建会话 → 输入模式 → "/dev" → Dev 对话框 → 输入目标 →
# Tab 到 Run → 启动（运行中视图）→ Esc 中断（出现恢复入口）。
#
# 意义：录屏不可复跑，这里用「可复跑的脚本 + 断言 + 抓取的屏幕文本」作为等效证据。
# 注意：dev 运行本身需要 LLM 才能出报告；本脚本只断言**界面状态迁移**，
# 不依赖 LLM 是否可用（无凭据时运行会失败，但界面迁移依然成立）。
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/tui-dev-smoke.sh [bin]

Drives the TUI in a pseudo-terminal and asserts the conversational-execution
flow: /dev dialog -> goal input -> Run -> running view -> Esc interrupt -> resume.
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

BIN="${1:-./target/debug/sisyphus}"
if [ ! -x "$BIN" ]; then
  echo "TUI binary not found: $BIN" >&2
  echo "build it first: cargo build -p sisyphus-cli" >&2
  exit 2
fi

python3 - "$BIN" <<'PY'
import fcntl
import os
import pty
import re
import select
import struct
import sys
import termios
import time

binary = sys.argv[1]
ANSI = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b[()][A-Z0-9]")

pid, fd = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"
    os.execv(
        binary,
        [binary, "--data-dir", "/tmp/opencode/tui-dev-smoke", "--bridge", "127.0.0.1:59999"],
    )
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))

buf = b""


def pump(seconds):
    global buf
    end = time.time() + seconds
    while time.time() < end:
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


def send(keys, wait=1.0):
    os.write(fd, keys)
    pump(wait)


def plain():
    return ANSI.sub("", buf.decode("utf-8", "replace"))


checks = []


def check(name, ok):
    checks.append((name, bool(ok)))


try:
    pump(2.5)
    send(b"\x0e", 2.0)          # Ctrl+N → 新建会话并进入会话视图
    send(b"i", 1.0)             # 进入输入模式
    send(b"/dev", 0.8)
    send(b"\r", 1.8)            # /dev → Dev 对话框
    check("dev_dialog_opens", "Dev Agent" in plain())

    send(b"smoke goal", 1.0)
    send(b"\t\t\t\t", 0.8)      # Tab 到 Run
    send(b"\r", 4.0)            # 启动
    check("running_view", "Running developer agent" in plain())

    send(b"\x1b", 2.5)          # Esc 中断
    check("resume_entry_after_interrupt", "resume" in plain().lower())
finally:
    try:
        os.write(fd, b"q")
    except OSError:
        pass
    time.sleep(0.3)
    try:
        os.kill(pid, 9)
    except ProcessLookupError:
        pass
    try:
        os.close(fd)
    except OSError:
        pass

for name, ok in checks:
    print(f"{'PASS' if ok else 'FAIL'}  {name}")
failed = [name for name, ok in checks if not ok]
if failed:
    sys.exit(f"TUI dev smoke failed: {', '.join(failed)}")
print(f"TUI dev smoke OK ({len(checks)} checks)")
PY
