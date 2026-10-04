BRIDGE_DEPS = crates/sisyphus-bridge/src/main.rs crates/sisyphus-bridge/Cargo.toml crates/sisyphus-bridge/proto/bridge.proto
# Build targets for crates excluded from workspace commands.
#
# Memory notes:
#   Full `cargo check --workspace` OOMs on <16GB machines due to wasmtime/cranelift deps.
#   Three crates pull in wasmtime: sisyphus-sandbox (direct), sisyphus-tests (direct),
#   and sisyphus-bridge (default features enable optional sandbox).
#
# Strategy: never use `cargo check --workspace` in CI. Instead:
#   - Lightweight batch: `--workspace --exclude <3 heavy crates>`
#   - Heavy crates get per-crate jobs (isolated, no OOM)
#
# sisyphus-bridge  — rustc 1.96.0 type-checking ICE (SIGTERM during `cargo check`).
#                    RESOLVED in nightly 1.98.0+; ships with 1.97.0 stable on 2026-07-09.
#                    While on 1.96.0: `make check-bridge-160` uses -C lto=off.
# sisyphus-sandbox — heavy wasmtime/cranelift deps; rarely needs recompiling.
# sisyphus-core   — wasmtime transitive dep; full test needs CARGO_BUILD_JOBS=1.
# sisyphus-tools-* — 10 sub-crates, lightweight, fast to check individually.
#
# 内存约束（2026-10-02 实测）：在 19.5 GiB 机器上并行链接多个重型测试二进制会
# OOM，报 `linking with cc failed` 或 `could not exec the linker cc`。因此所有会产生
# 多个测试二进制的目标都显式带 CARGO_BUILD_JOBS=1。直接敲 `cargo test` 不受此保护，
# 需自行加 -j 1。
#
# ⚠ 修正（2026-10-04）：旧注释写「`-j 2` 仍不安全，`-j 1` 稳定」。本轮实测发现
# **已带 `-j 1` 的 `make test-all` 仍出现过一次 SIGABRT**，同一条命令重跑即通过。
#
# 但**成因未定，不能说 `-j 1` 无效**：这里限制的是**构建级**并发
#（CARGO_BUILD_JOBS），而 `make test-all` 路径上**测试线程级**并发
#（`--test-threads`，默认 = CPU 核数）从未被限制。二者在观测上等价，
# 且该次 SIGABRT 的原始日志已被后续运行覆盖、无法再取证。
# 所以正确做法是：**遇到 SIGABRT 直接重跑并保留现场**，
# 要根治请先分别试 `--test-threads=1` 与 `-j 1` 做二分取证。
#
# Usage:
#   make check-all         # lightweight batch (alias: --exclude bridge/sandbox/tests)
#   make check-sandbox     # sandbox alone (wasmtime, isolated)
#   make check-tests       # integration tests alone (wasmtime, isolated)
#   make check-bridge      # bridge alone
#   make check-bridge-160 # check bridge only (1.96.0 fallback with -C lto=off)
#   make build-bridge     # build bridge only
#   make test-bridge      # test bridge only
#   make test-bridge-dedicated  # manual: ignore 标记的专用 runner 用例 (单任务/串行)
#   make test-core        # full sisyphus-core test suite
#   make test-core-quick  # lib-only sisyphus-core tests (~2s)
#   make test-tools       # test all 10 sisyphus-tools-* sub-crates (lib only, fast)
#   make check-tools      # check all 10 tools sub-crates


TOOLS_PACKAGES = \
	sisyphus-tools-agent sisyphus-tools-build sisyphus-tools-core \
	sisyphus-tools-executor sisyphus-tools-llm sisyphus-tools-llm-tools \
	sisyphus-tools-mcp sisyphus-tools-scm sisyphus-tools-session \
	sisyphus-tools-system

HEAVY_PACKAGES = sisyphus-sandbox sisyphus-sandboxd sisyphus-tests

.PHONY: all check-all build-all test-all scan-flaky flaky-replay rustc-abort-forensics
.PHONY: check-bridge build-bridge test-bridge check-bridge-160
.PHONY: check-bridge-nosandbox test-bridge-nosandbox
.PHONY: check-sandbox build-sandboxd test-sandbox check-sandboxd test-sandboxd test-tests check-tests
.PHONY: test-bridge-full test-bridge-dedicated test-cli-handlers
.PHONY: test-ci
.PHONY: perf-analyze
.PHONY: dev-tui run-tui clean
.PHONY: dev ci-quick dev-fmt dev-clippy dev-test ci-bridge

all: check-all

# — 一键开发入口（阶段 7.4 DX）—

# make dev: 完整本地验证流水线 (轻量级 + bridge)
# 跳过重型 crate (sandbox/tests)，按依赖顺序:
#   1) fmt 校验     → 防止风格漂移
#   2) clippy 严格  → 提前抓 lint
#   3) lib 单元测试  → 快速回归
#   4) bridge 单独   → 单独验证（重型 crate）
dev: ci-quick ci-bridge
	@echo ""
	@echo "✅ 完整本地验证通过（fmt + clippy + lib tests + bridge clippy）"

# make ci-quick: 不含重型 crate 的快速检查（默认开发循环）
ci-quick: dev-fmt dev-clippy dev-test
	@echo "✅ ci-quick: fmt + clippy + lib tests"

# make dev-fmt: 仅 fmt 校验
dev-fmt:
	@echo "━━━ 1/4 fmt check ━━━"
	cargo fmt --all --check

# make dev-clippy: 全工作区 clippy 严格（排除重型）
dev-clippy:
	@echo "━━━ 2/4 clippy ━━━"
	cargo clippy --workspace --exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude sisyphus-sandboxd --exclude sisyphus-tests -- -D warnings

# make dev-test: lib 单元测试
dev-test:
	@echo "━━━ 3/4 lib tests ━━━"
	cargo test --workspace --exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude sisyphus-sandboxd --exclude sisyphus-tests --lib

# make ci-bridge: bridge 单独 clippy（重型 crate 隔离）
ci-bridge:
	@echo "━━━ 4/4 bridge clippy ━━━"
	cargo clippy -p sisyphus-bridge -- -D warnings

# — workspace commands (exclude heavy crates) —
#
# 2026-10-04 补：`check-all` / `build-all` 原先没带 CARGO_BUILD_JOBS=1，
# 与 `test-all` 不一致。实测本机会在 `cargo check-all` 默认并行下撞
# rustc SIGABRT（ICE 标记 `0x0`），与 Makefile 头部记录的 OOM 现象同源。
# `check` 虽然不链接最终二进制，但 rustc 前端的 codegen 峰值同样吃内存。
#
# ⚠ 实测修正（2026-10-04）：`test-all` **已带** `-j 1` 仍会随机 SIGABRT，
# 同一条命令重跑即通过。所以这**不是**并发 OOM，而是概率性崩溃，
# `-j 1` 不能根除它。CARGO_BUILD_JOBS=1 保留（避开已知最坏情况），
# 但遇到 SIGABRT 时的正确处置是**直接重跑**，而不是继续调并发参数。

check-all:
	CARGO_BUILD_JOBS=1 cargo check-all

build-all:
	CARGO_BUILD_JOBS=1 cargo build-all

test-all:
	CARGO_BUILD_JOBS=1 cargo test-all

# 概率性失败（flaky test）扫描：把每个 crate 的 lib 测试二进制重复跑多轮，
# 统计失败率。单次 `cargo test` 全绿无法证明不存在竞态——本项目多处缺陷
# 失败率都在 1/16 以下，只在高频复跑下才暴露。
#
# 用法：make scan-flaky ROUNDS=32
scan-flaky:
	CARGO_BUILD_JOBS=1 cargo test --workspace --lib --no-run
	python3 scripts/scan_flaky.py --rounds $(or $(ROUNDS),16)

# 仅重跑上次扫描抓到的失败现场（现场已落盘，不必重新扫描）。
flaky-replay:
	@test -d "$(or $(REPORT_DIR),$$JCODE_SCRATCH_DIR/flaky-reports)" || \
		{ echo "没有现场目录；先跑 make scan-flaky"; exit 2; }
	@ls -1 "$(or $(REPORT_DIR),$$JCODE_SCRATCH_DIR/flaky-reports)"/*.log 2>/dev/null || true

# — rustc 随机 SIGABRT 取证 —
#
# 背景：实测出现过 `rustc --crate-name sisyphus_core ... (signal: 6, SIGABRT)`，
# 重跑即过，**成因至今未定**（尚未排除编译期内存压力 / rustc 内部并行等）。
# 每次想查都查不到的根本原因：`ulimit -c` 默认是 0，崩溃**不留任何现场**。
#
# 用法：make rustc-abort-forensics            # 默认 5 轮
#       make rustc-abort-forensics ROUNDS=20  # 加轮次
#       JOBS=4 make rustc-abort-forensics     # 改变构建并发（默认 1，即复刻 make test-all 场景）
#
# 原理：每一轮 `touch` 一个 crate 内源文件强制 rustc **真正重编 sisyphus-core**
#       （不加 touch 则 cargo 命中缓存直接返回，循环毫无意义）。
#       每次都在子 shell 内 `ulimit -c unlimited`；一旦复现，
#       现场由 systemd-coredump 接管，用 coredumpctl 事后分析。
#
# ⚠ 单轮可能耗时数分钟（sisyphus-core 依赖 wasmtime）。若只想快速看一眼，
#   用 ROUNDS=1；本目标**故意不用 -s（静默）之外的静默**，因为崩溃信息
#   必须落盘才有意义，故输出重定向到日志文件而非终端。
rustc-abort-forensics:
	@echo "=== 前置检查 ==="
	@if [ "$$(ulimit -c)" = "0" ]; then \
		echo "⚠  ulimit -c = 0；已在本目标子 shell 内覆盖为 unlimited"; \
	else echo "✓ ulimit -c = $$(ulimit -c)"; fi
	@echo "   core_pattern: $$(cat /proc/sys/kernel/core_pattern 2>/dev/null || echo 未知)"
	@command -v coredumpctl >/dev/null 2>&1 \
		&& echo "✓ coredumpctl 可用（复现后用 coredumpctl info <PID> 看栈）" \
		|| echo "⚠  coredumpctl 不可用；请人工检查 /var/lib/systemd/coredump/"
	@mkdir -p "$(or $(FORENSIC_DIR),$$JCODE_SCRATCH_DIR/forensics)"
	@echo "=== 循环重编 $(or $(ROUNDS),5) 轮（JOBS=$${JOBS:-1}，每轮强制 touch 重编）==="
	@fail=0; i=0; \
	while [ $$i -lt $(or $(ROUNDS),5) ]; do \
		i=$$((i+1)); \
		printf "  [%2d/%s] " "$$i" "$(or $(ROUNDS),5)"; \
		touch crates/sisyphus-core/src/lib.rs; \
		log="$(or $(FORENSIC_DIR),$$JCODE_SCRATCH_DIR/forensics)/round-$$i.log"; \
		if ( ulimit -c unlimited; CARGO_BUILD_JOBS=$${JOBS:-1} \
			cargo build -p sisyphus-core --tests ) >"$$log" 2>&1; then \
			echo "ok"; \
		else \
			echo "✗ 崩溃复现（第 $$i 轮）"; fail=1; \
			echo "--- 立即取证 ---"; \
			echo "    日志: $$log"; \
			coredumpctl list --no-pager 2>/dev/null | tail -3 \
				|| echo "    (coredumpctl 不可用，请人工检查 /var/lib/systemd/coredump/)"; \
			echo "    栈:   coredumpctl info <PID>"; \
			break; \
		fi; \
	done; \
	if [ $$fail -eq 0 ]; then \
		echo "=== $(or $(ROUNDS),5) 轮均未复现 —— 这是**阴性结果**，不证明成因 ==="; \
	fi
	@touch crates/sisyphus-core/src/lib.rs 2>/dev/null || true

# — 结构审计（不需要编译，纯静态扫描）——

# 孤儿源文件：src/ 下从未被 mod 引入的文件，不参与编译。
# 曾出现 crates/sisyphus-mem/src/tests.rs（227 行 7 个用例）从未被引用，
# 看似有覆盖实则从未运行过。
audit-orphans:
	@python3 scripts/audit_orphans.py

# 忽略测试的理由审计：#[ignore] 必须指向外部前置条件（网络/凭据/工具链/
# 运行中服务/已声明缺口），不能是「测试当前失败」。CI 不跑 --include-ignored，
# 被忽略即等于没有覆盖。
audit-ignored:
	@python3 scripts/audit_ignored.py

# 全部结构审计（不需编译）
audit: audit-orphans audit-ignored

.PHONY: audit-orphans audit-ignored audit

# — bridge crate (nightly+, no workaround needed) —

check-bridge: $(BRIDGE_DEPS)
	cargo check -p sisyphus-bridge

build-bridge: $(BRIDGE_DEPS)
	cargo build -p sisyphus-bridge

test-bridge: $(BRIDGE_DEPS)
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-bridge

check-bridge-nosandbox: $(BRIDGE_DEPS)
	cargo check -p sisyphus-bridge --no-default-features

test-bridge-nosandbox: $(BRIDGE_DEPS)
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-bridge --no-default-features --lib

test-bridge-full: $(BRIDGE_DEPS)
	cargo clippy -p sisyphus-bridge -- -D warnings
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-bridge --test bridge_integration_test

# — bridge dedicated-runner tests（#[ignore = "requires dedicated runner"] 用例）—
#
#   仅在专用运行环境手动执行，不进默认 CI/dev 流水线：
#   - CARGO_BUILD_JOBS=1  限制单编译任务，避免内存受限环境 OOM
#   - --no-default-features  不编译可选 sandbox 依赖
#   - -- --include-ignored --test-threads=1  跑全部用例且串行，避免共享状态互扰
#
#   用法：
#     make test-bridge-dedicated          # 全部用例（含 ignore）
#     make test-bridge-dedicated FILTER=test_execute_tool_strategic_next_goal_missing_project  # 单用例

test-bridge-dedicated: $(BRIDGE_DEPS)
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-bridge --no-default-features --test bridge_integration_test -- --include-ignored --test-threads=1 $(if $(FILTER),$(FILTER) --exact,)

test-cli-handlers:
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-cli --lib

check-stress-test:
	cargo check -p sisyphus-bridge --bin stress_test

# — core benchmarks (isolated, sandbox feature required) —

bench-core:
	cargo bench -p sisyphus-core --features sandbox -- --warm-up-time 1 --measurement-time 2 --sample-size 10

# — performance analysis (run benchmarks + critcmp comparison) —
#
#   Runs core benchmarks with --save-baseline current, then compares
#   against the previously saved main-baseline using critcmp.
#   If no baseline exists, saves current results as the baseline.
#
#   Prerequisites:
#     cargo install critcmp --locked   (first time)
#
#   Usage:
#     make perf-analyze                # compare with existing baseline
#     make perf-analyze BASELINE=myref # compare against a custom baseline name

perf-analyze:
	@echo "━━━ Running benchmarks (save as current) ━━━"
	cargo bench -p sisyphus-core --features sandbox -- --save-baseline current --warm-up-time 1 --measurement-time 2 --sample-size 10
	@BASELINE_NAME="${BASELINE:-main-baseline}"; \
	if ls target/criterion/*/"$$BASELINE_NAME"/ 2>/dev/null >/dev/null; then \
		echo "━━━ Comparing current vs $$BASELINE_NAME ━━━"; \
		if command -v critcmp >/dev/null 2>&1; then \
			critcmp current "$$BASELINE_NAME" 2>&1; \
			echo ""; \
			echo "━━━ Regression check (>5% = warning) ━━━"; \
			critcmp current "$$BASELINE_NAME" --json 2>/dev/null | python3 -c " \
import json,sys; data=json.load(sys.stdin); \
regs=[(n,i) for n,i in data.items() if isinstance(i,dict) and isinstance(i.get('change'),(int,float)) and i['change']>5.0]; \
[print(f'WARNING: {n}: {p[\"change\"]:.1f}% slower') for n,p in regs] if regs else print('No regressions >5%'); \
"; \
		else \
			echo "critcmp not installed. Install with: cargo install critcmp --locked"; \
		fi; \
	else \
		echo "━━━ No $$BASELINE_NAME found. Saving current as $$BASELINE_NAME ━━━"; \
		for dir in target/criterion/*/; do \
			if [ -d "$${dir}current" ]; then \
				cp -r "$${dir}current" "$${dir}$$BASELINE_NAME"; \
			fi; \
		done; \
		echo "Benchmark baseline saved as '$$BASELINE_NAME'"; \
	fi

# — E2E smoke test (bridge daemon + stress_test) —

e2e-smoke:
	@echo "━━━ Starting bridge in background ━━━"
	cargo build -p sisyphus-bridge
	cargo build -p sisyphus-bridge --bin stress_test
	@./scripts/e2e-smoke.sh

# — E2E /health/deep smoke test (启动 bridge → curl → 验证 JSON 结构) —

e2e-health: $(BRIDGE_DEPS)
	cargo build -p sisyphus-bridge
	cargo build -p sisyphus-bridge --bin stress_test
	@./scripts/e2e-health.sh

# — bridge crate, 1.96.0 fallback (RUSTFLAGS bypasses rustc 1.96.0 ICE) —

check-bridge-160: $(BRIDGE_DEPS)
	RUSTFLAGS="-C lto=off" cargo check -p sisyphus-bridge

# — sisyphus-tools-* sub-crates (10 independent, no heavy deps) —

test-tools:
	for pkg in $(TOOLS_PACKAGES); do \
		echo "━━━ testing $$pkg (lib)"; \
		CARGO_BUILD_JOBS=1 cargo test -p "$$pkg" --lib; \
	done

check-tools:
	for pkg in $(TOOLS_PACKAGES); do \
		echo "━━━ checking $$pkg"; \
		cargo check -p "$$pkg"; \
	done

# — sisyphus-core (wasmtime transitive dep needs CARGO_BUILD_JOBS=1) —

test-core:
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-core

test-core-quick:
	cargo test -p sisyphus-core --lib

test-core-min:
	cargo test -p sisyphus-core --lib --no-default-features

# — CI simulation —

test-ci-core:
	cargo fmt --all --check
	cargo clippy -p sisyphus-core
	cargo test -p sisyphus-core --lib
	cargo test -p sisyphus-core

# — dev workflow for TUI (fast edit → compile → test) —
#
#   Terminal 1: make dev-tui    # auto-builds on file change
#   Terminal 2: make run-tui    # (re)launch after build finishes
#
#   Or in one terminal with tmux:
#     tmux split-window -h 'make dev-tui'
#     make run-tui

SISYPHUS_CLI_SRC = crates/sisyphus-cli/src

dev-tui: $(SISYPHUS_CLI_SRC)
	@echo " Watching $(SISYPHUS_CLI_SRC)/ for changes — Ctrl+C to stop"
	@while inotifywait -q -r -e modify,create,delete,move $(SISYPHUS_CLI_SRC)/; do \
		echo "━━━ changed → cargo build -p sisyphus-cli"; \
		cargo build -p sisyphus-cli 2>&1 | tail -5; \
		echo ""; \
	done

run-tui:
	cargo run -p sisyphus-cli

# — sisyphus-sandbox (wasmtime, heavy, isolated) —

check-sandbox:
	cargo check -p sisyphus-sandbox

test-sandbox:
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-sandbox --lib

# — sisyphus-sandboxd (wasmtime via sandbox, heavy, isolated daemon) —

check-sandboxd:
	cargo check -p sisyphus-sandboxd

build-sandboxd:
	cargo build --release -p sisyphus-sandboxd

# cranelift-codegen 即使单任务也会把 rustc 撑爆（实测 rustc-LLVM OOM），
# 关掉调试信息才编得过。代价是该配置下需要全量重编。
test-sandboxd:
	CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 cargo test -p sisyphus-sandboxd

# — sisyphus-tests (wasmtime via sandbox, heavy, isolated) —

check-tests:
	cargo check -p sisyphus-tests

test-tests:
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-tests --lib

# — CI simulation (mirrors .github/workflows/ci.yml) —

test-ci:
	@echo "━━━ 1/14 Format ━━━"
	cargo fmt --all --check
	@echo "━━━ 2/14 Lint Light ━━━"
	cargo clippy --workspace --exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude sisyphus-sandboxd --exclude sisyphus-tests -- -D warnings
	@echo "━━━ 3/14 Lint Sandbox ━━━"
	cargo clippy -p sisyphus-sandbox -- -D warnings
	@echo "━━━ 4/14 Lint Sandboxd ━━━"
	cargo clippy -p sisyphus-sandboxd -- -D warnings
	@echo "━━━ 5/14 Lint Tests ━━━"
	cargo clippy -p sisyphus-tests -- -D warnings
	@echo "━━━ 6/14 Test Light ━━━"
	cargo test --workspace --exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude sisyphus-sandboxd --exclude sisyphus-tests
	@echo "━━━ 7/14 Test Core ━━━"
	cargo test -p sisyphus-core
	@echo "━━━ 8/14 Test Sandbox ━━━"
	cargo test -p sisyphus-sandbox --lib
	@echo "━━━ 9/14 Test Sandboxd ━━━"
	cargo test -p sisyphus-sandboxd
	@echo "━━━ 10/14 Bridge Full Test ━━━"
	cargo clippy -p sisyphus-bridge -- -D warnings
	cargo test -p sisyphus-bridge --test bridge_integration_test
	@echo "━━━ 11/14 Test CLI Handlers ━━━"
	cargo test -p sisyphus-cli
	@echo "━━━ 12/14 Test Integration ━━━"
	cargo test -p sisyphus-tests --lib
	@echo "━━━ 13/14 Bench Core ━━━"
	cargo bench -p sisyphus-core --features sandbox -- --warm-up-time 1 --measurement-time 2 --sample-size 10
	@echo "━━━ 14/14 CLI Build ━━━"
	cargo build --release -p sisyphus-cli
	@echo "━━━ CI simulation complete ━━━"

# — convenience —

# — agent evals (57 cases across metis/momus/librarian/scribe/security) —

eval:
	cargo run -p eval-runner

eval-check:
	cargo check -p eval-runner

clean:
	cargo clean
