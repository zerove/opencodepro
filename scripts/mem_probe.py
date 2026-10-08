#!/usr/bin/env python3
"""采样编译期间的内存峰值。

为什么不用 `/usr/bin/time -v`：它报的是**单个进程**的最大 RSS（取所有
waited-for children 里的最大值），而 OOM 风险来自**并发**——N 个 rustc 同时
各占几百 MB，峰值是总和。这个口径错误会让结论完全反过来：
测 `sisyphus-types` 得到 326 MB 看着很安全，但那只是单个 rustc；
真正吃内存的是 cranelift/wasmtime，且它们是并行的。

所以这里采样三个口径：
  1. rustc/rustdoc 进程 RSS 总和的峰值（并发驻留）
  2. 提交量的**增量**（相对编译前基线）
  3. 采样期间的 swap 变化（是否已换页到盘上，即内存真的不够）

注意：绝不能把 `Committed_AS` 的绝对值当成编译的影响——本机 IDE 的 7 个
java 进程就占了 22.9 GB（111% of limit），编译根本没参与。第一版就踩了这个坑，
得出了「编译导致 OOM」的错误结论。

用法：
    python3 scripts/mem_probe.py -- cargo check -p sisyphus-sandbox
    python3 scripts/mem_probe.py --jobs 1 -- cargo check -p sisyphus-sandbox
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import threading
import time
from pathlib import Path

SAMPLE_INTERVAL = 0.25  # 秒


def read_kb(field: str) -> int:
    try:
        with open("/proc/meminfo") as fh:
            for line in fh:
                if line.startswith(field):
                    return int(line.split()[1])
    except OSError:
        pass
    return 0


def compiler_rss_kb() -> tuple[int, int]:
    """返回 (rustc 类进程 RSS 总和 kB, 进程数)。"""
    total = 0
    count = 0
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            comm = (entry / "comm").read_text().strip()
            if comm not in ("rustc", "rustdoc", "clippy-driver", "sccache"):
                continue
            for line in (entry / "status").read_text().splitlines():
                if line.startswith("VmRSS:"):
                    total += int(line.split()[1])
                    count += 1
                    break
        except (OSError, ValueError, IndexError):
            continue
    return total, count


class PeakTracker:
    """采样编译带来的**增量**，而非绝对值。

    第一版把 `Committed_AS` 的绝对峰值当结论，错了：实测无任何编译进程时
    它已经达 22.9 GB（111% of limit），占用方是 IDE 的 7 个 java 进程
    （最大的 1.4 GB）。把基线算进峰值，会得出「编译导致 OOM」的错误结论，
    进而去调一个根本不相关的旋钮。

    所以这里同时记录基线（启动前）与峰值，只报告**增量**。
    基线本身超限的事实也值得知道——它说明真正该管的是 IDE 进程数，
    而不是编译并行度——所以一并打印，但不混进编译的账上。
    """

    def __init__(self) -> None:
        self.peak_rss = 0
        self.peak_count = 0
        self.peak_committed = 0
        self.samples = 0
        self.base_rss, _ = compiler_rss_kb()
        self.base_committed = read_kb("Committed_AS")
        self.swap_start = read_kb("SwapTotal")
        self.swap_free_start = read_kb("SwapFree")

    def sample(self) -> None:
        rss, count = compiler_rss_kb()
        committed = read_kb("Committed_AS")
        self.samples += 1
        if rss > self.peak_rss:
            self.peak_rss = rss
            self.peak_count = count
        if committed > self.peak_committed:
            self.peak_committed = committed

    def swap_delta_mb(self) -> int:
        used_start = self.swap_start - self.swap_free_start
        used_now = read_kb("SwapTotal") - read_kb("SwapFree")
        return (used_now - used_start) // 1024


def human(kb: int) -> str:
    return f"{kb / 1024:.0f} MB" if kb < 1024 * 1024 else f"{kb / 1024 / 1024:.2f} GB"


def main() -> int:
    parser = argparse.ArgumentParser(description="采样编译期间的内存峰值")
    parser.add_argument("--jobs", help="等价于 CARGO_BUILD_JOBS，仅用于打印")
    parser.add_argument("command", nargs=argparse.REMAINDER, help="-- 之后是要测的命令")
    args = parser.parse_args()

    command = args.command
    if command and command[0] == "--":
        command = command[1:]
    if not command:
        print("❌ 未指定命令。用法：python3 scripts/mem_probe.py -- cargo check ...")
        return 2

    tracker = PeakTracker()
    stop = threading.Event()

    def sampler() -> None:
        while not stop.is_set():
            tracker.sample()
            stop.wait(SAMPLE_INTERVAL)

    print(f"采样中：{' '.join(command)}")
    if args.jobs:
        print(f"（jobs={args.jobs}）")
    print()

    thread = threading.Thread(target=sampler, daemon=True)
    start = time.time()
    thread.start()
    try:
        result = subprocess.run(command)
    finally:
        stop.set()
        thread.join(timeout=2)
    elapsed = time.time() - start

    limit = read_kb("CommitLimit")
    delta_committed = max(0, tracker.peak_committed - tracker.base_committed)
    headroom = limit - tracker.base_committed

    print()
    print("── 编译带来的内存增量 ──")
    print(f"  rustc 并发驻留峰值 : {human(tracker.peak_rss)}（峰值时 {tracker.peak_count} 个进程同时在跑）")
    print(f"  提交量增量         : {human(delta_committed)}")
    print()
    print("── 环境基线（编译前已存在，不计入上面的账）──")
    print(f"  Committed_AS 基线  : {human(tracker.base_committed)} / 上限 {human(limit)}")
    if limit:
        pct = tracker.base_committed / limit * 100
        if pct > 100:
            print(f"  ⚠️  基线已超限 {pct:.1f}% —— 与编译无关，是常驻进程（IDE 等）造成的")
        else:
            print(f"  基线占限           : {pct:.1f}%，余量 {human(max(0, headroom))}")
        if delta_committed > headroom > 0:
            print(f"  ⚠️  编译增量 {human(delta_committed)} 已吃掉全部余量 {human(headroom)}")
            print("      → 这才是并发过高的真实风险；先减常驻进程，或调低并行度")
    print()
    print(f"  采样期间 swap 变化 : {tracker.swap_delta_mb():+d} MB")
    print(f"  采样点 / 耗时      : {tracker.samples} 个 / {elapsed:.1f}s")
    print(f"  命令退出码         : {result.returncode}")
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())