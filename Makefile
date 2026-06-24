# Build targets for crates excluded from workspace commands.
#
# sisyphus-bridge  — rustc 1.96.0 type-checking ICE (SIGTERM during `cargo check`).
#                    RESOLVED in nightly 1.98.0+ (will ship with 1.97.0 stable on 2026-07-09).
#                    Project override: `rustup override set nightly` already active.
#                    Fallback for 1.96.0: `make check-bridge-160` uses -C lto=off.
# sisyphus-sandbox — heavy wasmtime/cranelift deps; rarely needs recompiling for quick checks.
#
# Usage:
#   make check-all        # cargo alias → check --workspace --exclude bridge/sandbox (1.96.0-safe)
#   make check-bridge     # check bridge only (requires nightly+, no workaround flags)
#   make check-bridge-160 # check bridge only (1.96.0 fallback with -C lto=off)
#   make build-bridge     # build bridge only
#   make test-bridge      # test bridge only

BRIDGE_DEPS = crates/sisyphus-bridge/src/main.rs crates/sisyphus-bridge/build.rs crates/sisyphus-bridge/proto/bridge.proto

.PHONY: all check-all build-all test-all
.PHONY: check-bridge build-bridge test-bridge check-bridge-160
.PHONY: clean

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

# — bridge crate, 1.96.0 fallback (RUSTFLAGS bypasses rustc 1.96.0 ICE) —

check-bridge-160: $(BRIDGE_DEPS)
	RUSTFLAGS="-C lto=off" cargo check -p sisyphus-bridge

# — convenience —

clean:
	cargo clean
