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
    python3 scripts/mem_probe.py --self-check

注意：--jobs 传的是 CARGO_BUILD_JOBS，但若 .cargo/config.toml 里已有
[build] jobs，该值优先级更高。已实测本仓库当前无此配置项。
"""

from __future__ import annotations

import argparse
import os
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
            if comm not in COMPILER_COMMS:
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


COMPILER_COMMS = ("rustc", "rustdoc", "clippy-driver", "sccache")


def self_check() -> int:
    """验证采样器的两个关键性质。

    第一版把 Committed_AS 绝对值当结论，得出「编译导致超限」的错误结论；
    又因为没有自检，后来把 /proc 解析写错也没人发现。这里把两条都钉住。
    """
    failures = 0
    print("采样器自检：")

    # 1. /proc/meminfo 解析：字段缺失时应返回 0 而非抛异常
    total = read_kb("MemTotal")
    bogus = read_kb("NoSuchFieldHere")
    ok = total > 0 and bogus == 0
    failures += 0 if ok else 1
    print(f"  {'✅' if ok else '❌'} meminfo 解析      MemTotal={total} kB, 缺失字段={bogus}")

    # 2. 进程白名单：必须包含真实存在的编译器进程名。
    #    实测真实编译中的 comm 就是纯 "rustc"（不带版本/后缀），
    #    这一点曾用 python 进程做阳性对照代替，不能算数。
    ok = "rustc" in COMPILER_COMMS
    failures += 0 if ok else 1
    print(f"  {'✅' if ok else '❌'} 进程白名单      {COMPILER_COMMS}")

    # 3. 采样器必须能在真有进程时返回非零 RSS（而不是恒返回 0）。
    #    第一版的输出里 rustc 一栏曾长期是 "0 MB"，而没人质疑。
    rss, count = compiler_rss_kb()
    print(f"  ℹ️  当前环境: rustc 类进程 {count} 个, RSS {human(rss)}")
    if count == 0:
        print("      （此刻无编译在跑，属正常；真实验证需在编译中执行）")

    # 4. human() 的单位换算：阈值两侧都要验。
    #    注意 1048576 kB 恰好跨过 1 GiB 阈值，走的是 GB 分支。
    unit_cases = ((0, "0 MB"), (1024, "1 MB"), (1024 * 1024 - 1, "1024 MB"), (1024 * 1024, "1.00 GB"))
    for value, want in unit_cases:
        got = human(value)
        ok = got == want
        failures += 0 if ok else 1
        print(f"  {'✅' if ok else '❌'} human({value}) = {got}")

    if failures:
        print(f"\n❌ 自检失败：{failures} 项")
        return 1
    print("\n✅ 自检通过")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="采样编译期间的内存峰值")
    parser.add_argument("--jobs", type=int, help="实际设置 CARGO_BUILD_JOBS（此前只打印不生效）")
    parser.add_argument("--self-check", action="store_true", help="只跑采样器自检")
    parser.add_argument("command", nargs=argparse.REMAINDER, help="-- 之后是要测的命令")
    args = parser.parse_args()

    if args.self_check:
        return self_check()

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

    env = os.environ.copy()
    if args.jobs:
        # 之前这个参数只打印不动，导致「-j 1 和 -j 12 跑了同一个命令」这种
        # 结论看起来成立其实根本没测到。现在必须真正传下去。
        env["CARGO_BUILD_JOBS"] = str(args.jobs)

    print(f"采样中：{' '.join(command)}")
    if args.jobs:
        print(f"（已设置 CARGO_BUILD_JOBS={args.jobs}，cargo 实际会用此并行度）")
    print()

    thread = threading.Thread(target=sampler, daemon=True)
    start = time.time()
    thread.start()
    try:
        result = subprocess.run(command, env=env)
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