#!/usr/bin/env bash
# P1-F: 单机契约冒烟（可执行子集）。按环境可用性运行；失败需区分回归与环境限制。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PASS=0 FAIL=0 SKIP=0
step() { printf '\n==> %s\n' "$*"; }
ok()   { PASS=$((PASS+1)); printf '  [PASS] %s\n' "$*"; }
bad()  { FAIL=$((FAIL+1)); printf '  [FAIL] %s\n' "$*" >&2; }
skip() { SKIP=$((SKIP+1)); printf '  [SKIP] %s\n' "$*"; }

run() {
  local name="$1"; shift
  if "$@" >/tmp/sn-smoke.out 2>&1; then ok "$name"; else bad "$name (see /tmp/sn-smoke.out)"; tail -20 /tmp/sn-smoke.out >&2 || true; fi
}

step "Rust 门禁（基础）"
run "cargo fmt --check" env -u RUSTUP_TOOLCHAIN cargo fmt --all --check
run "cargo check-all" env -u RUSTUP_TOOLCHAIN cargo check-all
run "make audit" make audit

step "Rust 测试（单机关键）"
run "cargo test -p sisyphus-core --lib" env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-core --lib
run "cargo test -p sisyphus-webui" env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-webui

step "CLI 本地冒烟"
run "sisyphus exec" env -u RUSTUP_TOOLCHAIN cargo run -q -p sisyphus-cli -- exec 'echo smoke-ok'
TMPD=$(mktemp -d)
if env -u RUSTUP_TOOLCHAIN cargo run -q -p sisyphus-cli -- --data-dir "$TMPD" session create smoke >/tmp/sn-smoke.out 2>&1 \
   && env -u RUSTUP_TOOLCHAIN cargo run -q -p sisyphus-cli -- --data-dir "$TMPD" session list >/tmp/sn-smoke.out 2>&1; then
  ok "cli session create/list"
else
  bad "cli session create/list"; tail -20 /tmp/sn-smoke.out >&2 || true
fi
rm -rf "$TMPD"

step "SDK 冒烟（按环境可用性）"
if command -v go >/dev/null 2>&1; then
  run "go-sdk test" bash -lc 'cd ide/go-sdk && go test ./...'
else
  skip "go-sdk (go not found)"
fi
if command -v python >/dev/null 2>&1 || command -v python3 >/dev/null 2>&1; then
  PY=${PYTHON:-python3}
  run "python-sdk pytest" bash -lc "cd ide/python-sdk && $PY -m pytest tests/ -q"
else
  skip "python-sdk (python not found)"
fi
if command -v npm >/dev/null 2>&1 && [ -d ide/typescript-sdk/node_modules ]; then
  run "ts-sdk test" bash -lc 'cd ide/typescript-sdk && npm run build && npm test'
else
  skip "ts-sdk (npm/node_modules unavailable)"
fi

step "Bridge 运行时就绪冒烟（可选）"
if [ -n "${SMOKE_BRIDGE_HTTP:-}" ]; then
  if BASE="$SMOKE_BRIDGE_HTTP" WS="${SMOKE_BRIDGE_WS:-default}" TIMEOUT="${SMOKE_BRIDGE_TIMEOUT:-5}" \
    ./scripts/single-node-readiness-check.sh >/tmp/sn-smoke.out 2>&1; then
    ok "bridge readiness ($SMOKE_BRIDGE_HTTP)"
  else
    bad "bridge readiness ($SMOKE_BRIDGE_HTTP)"; tail -20 /tmp/sn-smoke.out >&2 || true
  fi
else
  skip "bridge readiness (set SMOKE_BRIDGE_HTTP to enable)"
fi

printf '\n==> Summary: PASS=%d FAIL=%d SKIP=%d\n' "$PASS" "$FAIL" "$SKIP"
if [ "$FAIL" -gt 0 ]; then exit 1; fi
