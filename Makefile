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
# Usage:
#   make check-all         # lightweight batch (alias: --exclude bridge/sandbox/tests)
#   make check-sandbox     # sandbox alone (wasmtime, isolated)
#   make check-tests       # integration tests alone (wasmtime, isolated)
#   make check-bridge      # bridge alone
#   make check-bridge-160 # check bridge only (1.96.0 fallback with -C lto=off)
#   make build-bridge     # build bridge only
#   make test-bridge      # test bridge only
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

.PHONY: all check-all build-all test-all
.PHONY: check-bridge build-bridge test-bridge check-bridge-160
.PHONY: check-bridge-nosandbox test-bridge-nosandbox
.PHONY: check-sandbox build-sandboxd test-sandbox check-sandboxd test-tests check-tests
.PHONY: test-bridge-full test-cli-handlers
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

check-all:
	cargo check-all

build-all:
	cargo build-all

test-all:
	cargo test-all

# — bridge crate (nightly+, no workaround needed) —

check-bridge: $(BRIDGE_DEPS)
	cargo check -p sisyphus-bridge

build-bridge: $(BRIDGE_DEPS)
	cargo build -p sisyphus-bridge

test-bridge: $(BRIDGE_DEPS)
	cargo test -p sisyphus-bridge

check-bridge-nosandbox: $(BRIDGE_DEPS)
	cargo check -p sisyphus-bridge --no-default-features

test-bridge-nosandbox: $(BRIDGE_DEPS)
	cargo test -p sisyphus-bridge --no-default-features --lib

test-bridge-full: $(BRIDGE_DEPS)
	cargo clippy -p sisyphus-bridge -- -D warnings
	cargo test -p sisyphus-bridge --test bridge_integration_test

test-cli-handlers:
	cargo test -p sisyphus-cli --lib

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
		cargo test -p "$$pkg" --lib; \
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
	cargo test -p sisyphus-sandbox --lib

# — sisyphus-sandboxd (wasmtime via sandbox, heavy, isolated daemon) —

check-sandboxd:
	cargo check -p sisyphus-sandboxd

build-sandboxd:
	cargo build --release -p sisyphus-sandboxd

# — sisyphus-tests (wasmtime via sandbox, heavy, isolated) —

check-tests:
	cargo check -p sisyphus-tests

test-tests:
	cargo test -p sisyphus-tests --lib

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
