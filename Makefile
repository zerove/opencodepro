# Build targets for crates excluded from workspace commands.
#
# Memory notes:
#   Full `cargo check --workspace` OOMs on <16GB machines due to wasmtime/cranelift deps.
#   Workaround: `CARGO_BUILD_JOBS=1` or check individual crates.
#
# sisyphus-bridge  — rustc 1.96.0 type-checking ICE (SIGTERM during `cargo check`).
#                    RESOLVED in nightly 1.98.0+; ships with 1.97.0 stable on 2026-07-09.
#                    While on 1.96.0: `make check-bridge-160` uses -C lto=off.
# sisyphus-sandbox — heavy wasmtime/cranelift deps; rarely needs recompiling.
# sisyphus-core   — wasmtime transitive dep; full test needs CARGO_BUILD_JOBS=1.
# sisyphus-tools-* — 10 sub-crates, lightweight, fast to check individually.
#
# Usage:
#   make check-all        # cargo alias → check --workspace --exclude bridge/sandbox (1.96.0-safe)
#   make check-bridge     # check bridge only (requires nightly+, no workaround flags)
#   make check-bridge-160 # check bridge only (1.96.0 fallback with -C lto=off)
#   make build-bridge     # build bridge only
#   make test-bridge      # test bridge only
#   make test-core        # full sisyphus-core test suite
#   make test-core-quick  # lib-only sisyphus-core tests (~2s)
#   make test-tools       # test all 10 sisyphus-tools-* sub-crates (lib only, fast)
#   make check-tools      # check all 10 tools sub-crates

BRIDGE_DEPS = crates/sisyphus-bridge/src/main.rs crates/sisyphus-bridge/build.rs crates/sisyphus-bridge/proto/bridge.proto

TOOLS_PACKAGES = \
	sisyphus-tools-agent sisyphus-tools-build sisyphus-tools-core \
	sisyphus-tools-executor sisyphus-tools-llm sisyphus-tools-llm-tools \
	sisyphus-tools-mcp sisyphus-tools-scm sisyphus-tools-session \
	sisyphus-tools-system

.PHONY: all check-all build-all test-all
.PHONY: check-bridge build-bridge test-bridge check-bridge-160
.PHONY: test-tools check-tools
.PHONY: dev-tui run-tui clean

all: check-all

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

# — convenience —

clean:
	cargo clean
