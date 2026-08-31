# Sisyphus justfile — 跨平台开发命令镜像
# 用法: `just <recipe>` (需先 `cargo install just`)
# 镜像 Makefile，目标是 Windows / macOS / Linux 一致体验。
# 当 make 的 tab 缩进 / shell 差异成为瓶颈时优先用 just。
# 不设置 `set shell` 以保留各平台默认 shell 行为
# (Linux/macOS 用 bash，Windows 用 cmd.exe)。

# ── 默认 ───────────────────────────────────────────

# 显示所有可用命令
default:
    @just --list

# ── 一键开发入口（阶段 7.4）──

# 完整本地验证流水线：fmt + clippy + lib tests + bridge
dev: ci-quick ci-bridge
    @echo ""
    @echo "✅ 完整本地验证通过"

# 快速检查（不含重型 crate）
ci-quick: dev-fmt dev-clippy dev-test
    @echo "✅ ci-quick: fmt + clippy + lib tests"

# ── 单步检查 ────────────────────────────────────────

# cargo fmt --all --check
dev-fmt:
    @echo "━━━ 1/4 fmt check ━━━"
    cargo fmt --all --check

# cargo clippy -D warnings（排除重型 crate）
dev-clippy:
    @echo "━━━ 2/4 clippy ━━━"
    cargo clippy --workspace --exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude sisyphus-sandboxd --exclude sisyphus-tests -- -D warnings

# cargo test --lib（排除重型 crate）
dev-test:
    @echo "━━━ 3/4 lib tests ━━━"
    cargo test --workspace --exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude sisyphus-sandboxd --exclude sisyphus-tests --lib

# bridge 单独 clippy（重型 crate 隔离）
ci-bridge:
    @echo "━━━ 4/4 bridge clippy ━━━"
    cargo clippy -p sisyphus-bridge -- -D warnings

# ── 桥接 crate 专用（Makefile 中的别名）────────────

# 检查 bridge（含 wasmtime 重型依赖）
check-bridge:
    cargo check -p sisyphus-bridge

# 构建 bridge
build-bridge:
    cargo build -p sisyphus-bridge

# 测试 bridge
test-bridge:
    cargo test -p sisyphus-bridge

# ── 沙箱 crate 专用 ────────────────────────────────

# 检查 sandbox（wasmtime 重型）
check-sandbox:
    cargo check -p sisyphus-sandbox

# ── 核心 crate 专用 ────────────────────────────────

# 核心 lib 测试（不含重型）
test-core-quick:
    cargo test -p sisyphus-core --lib

# 核心基准测试
bench-core:
    cargo bench -p sisyphus-core --features sandbox -- --warm-up-time 1 --measurement-time 2 --sample-size 10

# ── 工具子 crate ───────────────────────────────────

# 检查全部 10 个 tools-* 子 crate
check-tools:
    for pkg in sisyphus-tools-agent sisyphus-tools-build sisyphus-tools-core sisyphus-tools-executor sisyphus-tools-llm sisyphus-tools-llm-tools sisyphus-tools-mcp sisyphus-tools-scm sisyphus-tools-session sisyphus-tools-system; do echo "━━━ checking $pkg"; cargo check -p $pkg; done

# 测试全部 10 个 tools-* 子 crate
test-tools:
    for pkg in sisyphus-tools-agent sisyphus-tools-build sisyphus-tools-core sisyphus-tools-executor sisyphus-tools-llm sisyphus-tools-llm-tools sisyphus-tools-mcp sisyphus-tools-scm sisyphus-tools-session sisyphus-tools-system; do echo "━━━ testing $pkg"; cargo test -p $pkg --lib; done

# ── TUI 工作流 ─────────────────────────────────────

# 监听源码变更自动构建 TUI（需 inotifywait）
dev-tui:
    @echo " Watching sisyphus-cli/src/ for changes — Ctrl+C to stop"
    @while inotifywait -q -r -e modify,create,delete,move crates/sisyphus-cli/src/; do echo "━━━ changed → cargo build -p sisyphus-cli"; cargo build -p sisyphus-cli 2>&1 | tail -5; echo ""; done

# 启动 TUI
run-tui:
    cargo run -p sisyphus-cli

# ── 清理 ───────────────────────────────────────────

# 清理构建产物（警告：耗时重建）
clean:
    cargo clean
