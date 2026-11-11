#!/usr/bin/env python3
"""重复运行测试二进制，筛查概率性失败（flaky test）。

背景：概率性失败无法靠"单次全量通过"证明不存在。本项目在一次单次全绿之后，
仍连续发现三处真实缺陷：

- `meta_learner::tests::test_learn_empty` 读开发者本机的 `.omo/meta`（被 gitignore），
  本机有历史样本就必失败，而 CI 永远绿；
- `feedback.rs` 的临时目录计数器跨进程从 0 重置，上次残留被本次复用；
- `config_store` / `http_client` 的进程级 env 竞争。

这三处的失败率都在 1/16 ~ 15/15 之间，单次运行几乎不可能撞见。
本脚本重复采样，把"偶尔红"变成可度量的数字。

用法：
    python3 scripts/scan_flaky.py                # 默认 16 轮
    python3 scripts/scan_flaky.py --rounds 40    # 提高采样
    python3 scripts/scan_flaky.py --keep-going   # 某 crate 失败后继续跑完其余 crate

CI：.github/workflows/flaky-scan.yml 每天定时（UTC 06:00）并支持手动触发，
手动时可调 rounds/jobs。该 job 刻意**不**挂在 PR 上——受控 runner 负载稳定，
恰恰是低频缺陷最难复现的环境，当门禁只会制造"扫了但扫不出"的安全感。

退出码：
    0 = 全部通过
    1 = 发现概率性失败（失败现场已写入 $JCODE_SCRATCH_DIR/flaky-reports）
    2 = 一个测试二进制都没找到。这不是"通过"，而是"根本没跑"——
       若返回 0，CI 会把未执行误读成全绿，是最危险的假绿灯。
"""

import argparse
import concurrent.futures
import json
import os
import re
import subprocess
import sys

# 这些 crate 的测试会真实执行 `cargo`（编译临时 crate）、开网络连接或监听文件，
# 单次可达数分钟，且失败高度依赖机器负载。放进高频循环既拖慢扫描，又只会
# 测出资源竞争而非代码缺陷。由 `make test-all` 覆盖。
SLOW_CRATES = {
    "sisyphus_core",
    "sisyphus_cli",
    "sisyphus_sandbox",
    "sisyphus_tests",
    "sisyphus_tools_build",
    "sisyphus_tools_llm_tools",
    "sisyphus_tools_session",
    "bridge_integration",
    "cluster_integration",
    # bridge 的 lib 测试含 sandboxd 守护进程用例：本机没有 `sisyphus-sandboxd`
    # 二进制时**确定性**失败（spawn: Permission denied），会被本脚本误报成
    # 「概率性失败」。仓库约定 bridge 测试不进常规 CI（OOM），故整体排除。
    "sisyphus_bridge",
}

# 单个测试二进制的超时（秒）。留足余量：慢 crate 已被排除。
PER_RUN_TIMEOUT = 180


def workspace_lib_binaries(target_dir: str):
    """列出工作区各 crate 的 lib 测试二进制。

    通过 `cargo metadata` 取权威 crate 列表，而不是 glob `target/debug/deps/*`——
    后者会混入历史残留的旧二进制（本项目曾因此把修复前的旧产物当成修复版来测，
    得出完全相反的结论）。每个 crate 只取 mtime 最新的一个。

    target 目录同样取自 metadata，不硬编码 `target/debug/deps`：CARGO_TARGET_DIR
    或 profile 变体都会改变该路径，硬编码会静默扫到空目录并报告"零失败"——
    那是最危险的假绿灯。
    """
    meta = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True,
        text=True,
        check=True,
    )
    parsed = json.loads(meta.stdout)
    packages = parsed["packages"]

    if not os.path.isdir(target_dir):
        target_dir = parsed["target_directory"]
    deps_dir = os.path.join(target_dir, "debug", "deps")
    # 新版 cargo 把测试二进制放到 `target/<profile>/build/<pkg>/<hash>/out/`，
    # 旧产物仍留在 `target/<profile>/deps/`。只搜 deps/ 会漏掉**新构建**的二进制：
    # 本机实测 types/metrics/tools-agent/bridge 的新产物都在 build/*/out 下，
    # deps/ 里只剩更早的旧产物 → 这些 crate 被判为「产物早于源码」而**静默跳过**，
    # 扫描却报告「全部通过」——正是本脚本注释里警告的最危险假绿灯。
    # 因此两处都要搜，并按 mtime 取最新。
    search_dirs = []
    if os.path.isdir(deps_dir):
        search_dirs.append(deps_dir)
    build_root = os.path.join(target_dir, "debug", "build")
    if os.path.isdir(build_root):
        # 层级是 build/<pkg>/<hash>/out/（两层），不是 build/<pkg>/out/
        for pkg_dir in sorted(os.listdir(build_root)):
            pkg_path = os.path.join(build_root, pkg_dir)
            if not os.path.isdir(pkg_path):
                continue
            for hash_dir in sorted(os.listdir(pkg_path)):
                out_dir = os.path.join(pkg_path, hash_dir, "out")
                if os.path.isdir(out_dir):
                    search_dirs.append(out_dir)
    if not search_dirs:
        print("找不到测试产物目录: %s" % deps_dir, file=sys.stderr)
        return []
    out = []
    stale = []
    for pkg in packages:
        crate = pkg["name"].replace("-", "_")
        if crate in SLOW_CRATES:
            continue
        pattern = re.compile(re.escape(crate) + r"-[0-9a-f]{16}$")
        candidates = []
        for search_dir in search_dirs:
            try:
                found = [f for f in os.listdir(search_dir) if pattern.fullmatch(f)]
            except FileNotFoundError:
                continue
            for f in found:
                candidates.append((os.path.getmtime(os.path.join(search_dir, f)), search_dir, f))
        if not candidates:
            continue
        _, newest_dir, newest = max(candidates, key=lambda item: item[0])
        binary = os.path.join(newest_dir, newest)
        # 确认它真的是测试二进制（含 `#[test]` 用例）
        probe = subprocess.run(
            [binary, "--list"], capture_output=True, text=True, timeout=60
        )
        if probe.returncode == 0 and ": test" in probe.stdout:
            # 产物必须不早于该 crate 的源码，否则扫的是陈旧二进制。
            # 仅按 mtime 取"最新"并不够：pre-commit hook 重编译会让另一个哈希
            # 变体更新，而它对应的源码可能已被改过。本项目曾因此把修复前的旧
            # 产物当成修复版来测，得出完全相反的结论。
            src_root = os.path.join(os.path.dirname(pkg["manifest_path"]), "src")
            if os.path.isdir(src_root):
                built_at = os.path.getmtime(binary)
                newest_src = max(
                    os.path.getmtime(os.path.join(dirpath, f))
                    for dirpath, _, files in os.walk(src_root)
                    for f in files
                    if f.endswith(".rs")
                )
                if built_at + 1 < newest_src:
                    stale.append(crate)
                    continue
            out.append(binary)
    if stale:
        print("以下 crate 的测试产物早于源码，已跳过（请先重新编译）：%s"
              % ", ".join(stale), file=sys.stderr)
    return sorted(out)


def run_once(binary: str, threads: int):
    try:
        proc = subprocess.run(
            [binary, "--test-threads", str(threads)],
            capture_output=True,
            text=True,
            timeout=PER_RUN_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return 124, "TIMEOUT after %ds" % PER_RUN_TIMEOUT
    return proc.returncode, (proc.stdout + proc.stderr)


def failed_tests(output: str):
    return [
        line.split()[1]
        for line in output.splitlines()
        if line.startswith("test ") and line.rstrip().endswith("FAILED")
    ]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rounds", type=int, default=16, help="每个二进制重复次数")
    ap.add_argument("--threads", type=int, default=16, help="每个二进制的测试线程数")
    ap.add_argument("--jobs", type=int, default=4, help="同时压测几个二进制")
    ap.add_argument("--keep-going", action="store_true", help="失败后继续扫完其余二进制")
    args = ap.parse_args()

    target_dir = os.environ.get("CARGO_TARGET_DIR", "target")
    binaries = workspace_lib_binaries(target_dir)
    if not binaries:
        # 扫描不到任何二进制时必须失败。若返回 0，CI 会把"根本没跑"
        # 误读成"全部通过"——这是最危险的假绿灯。
        print("未找到测试二进制；先运行 `cargo test --workspace --lib --no-run`",
              file=sys.stderr)
        return 2

    print("参与扫描: %d 个 crate × %d 轮（已排除 %d 个慢 crate）"
          % (len(binaries), args.rounds, len(SLOW_CRATES)), flush=True)

    failures = {}          # binary -> {round_index: output}
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = {
            pool.submit(run_once, b, args.threads): (b, r)
            for r in range(args.rounds)
            for b in binaries
        }
        done = 0
        for future in concurrent.futures.as_completed(futures):
            binary, rnd = futures[future]
            done += 1
            try:
                rc, output = future.result()
            except Exception as exc:                      # noqa: BLE001
                rc, output = 1, "harness error: %r" % (exc,)
            if rc != 0:
                failures.setdefault(binary, {})[rnd] = output
                if not args.keep_going:
                    print("发现失败，提前中止（用 --keep-going 扫完全部）", flush=True)
                    break
            if done % 20 == 0 or done == len(futures):
                print("  进度 %d/%d" % (done, len(futures)), flush=True)

    if not failures:
        print("全部通过：未发现概率性失败")
        return 0

    # 失败输出必须落盘。低频缺陷在事后往往无法立刻复现（本项目实测：
    # 扫描时抓到 1 次，随后 60 次复跑全绿），现场丢失就等于线索永久灭失。
    out_dir = os.path.join(
        os.environ.get("JCODE_SCRATCH_DIR", "/tmp"), "flaky-reports"
    )
    os.makedirs(out_dir, exist_ok=True)

    print("\n===== 发现概率性失败 =====")
    for binary, hits in sorted(failures.items()):
        stem = os.path.basename(binary)
        print("%s  %d/%d" % (stem, len(hits), args.rounds))
        sample = hits[min(hits)]
        for rnd in sorted(hits):
            path = os.path.join(out_dir, "%s.round%d.log" % (stem, rnd))
            with open(path, "w") as fh:
                fh.write(hits[rnd])
        print("    现场已保存: %s" % out_dir)
        for name in failed_tests(sample)[:5]:
            print("    %s" % name)
        for line in sample.splitlines():
            if line.strip().startswith(("left:", "right:", "error", "TIMEOUT")):
                print("    %s" % line.strip()[:160])
    print("\n提示：低频缺陷在受控环境常无法复现，请核对扫描期间的系统负载。")
    return 1


if __name__ == "__main__":
    sys.exit(main())
