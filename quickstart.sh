#!/usr/bin/env bash
set -euo pipefail

SISY_ROOT="$(cd "$(dirname "$0")" && pwd)"
SISY_BIN="${SISY_ROOT}/target/release/sisyphus"
SISY_DATA="${SISY_ROOT}/.sisyphus"

# ── Colors ──────────────────────────────────────────────
BOLD='\033[1m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m'

FORCE_BUILD=false
WITH_BRIDGE=false

# ── Parse flags ──────────────────────────────────────────
args=()
for arg in "$@"; do
    case "$arg" in
        -f|--force) FORCE_BUILD=true ;;
        --with-bridge) WITH_BRIDGE=true ;;
        *) args+=("$arg") ;;
    esac
done
set -- "${args[@]}"

# ── Help ────────────────────────────────────────────────
usage() {
    cat <<EOF
${BOLD}Sisyphus — Quick Start${NC}

${CYAN}Usage:${NC}
  ./quickstart.sh                   Build & launch TUI
  ./quickstart.sh --with-bridge     Build & launch TUI with bridge daemon
  ./quickstart.sh -f                Force rebuild then launch TUI
  ./quickstart.sh help              Show this help
  ./quickstart.sh <command>         Run any sisyphus CLI command

${CYAN}Flags:${NC}
  -f, --force        Force rebuild even if binary already exists
  --with-bridge      Auto-start bridge daemon alongside TUI (for SOP/Roadmap panels)

${CYAN}Examples:${NC}
  ./quickstart.sh                        # → sisyphus -i
  ./quickstart.sh --with-bridge          # → bridge + sisyphus -i
  ./quickstart.sh -f                     # → rebuild + sisyphus -i
  ./quickstart.sh todo list              # → sisyphus todo list
  ./quickstart.sh wisdom list            # → sisyphus wisdom list
  ./quickstart.sh session list           # → sisyphus session list
  ./quickstart.sh exec "cargo test"      # → sisyphus exec "cargo test"

${CYAN}Workflows:${NC}
  # 1. Full stack: start bridge + TUI together
  ./quickstart.sh --with-bridge

  # 2. Quick check — run all workspace tests
  ./quickstart.sh exec "cargo test --workspace"

  # 3. Manage tasks
  ./quickstart.sh todo add "Fix login bug"
  ./quickstart.sh todo list

  # 4. Save knowledge
  ./quickstart.sh wisdom record conventions "Use Result<T> not unwrap()"

  # 5. Start bridge daemon alone (background)
  ./quickstart.sh daemon &

${CYAN}Data directory:${NC}  ${SISY_DATA}
${CYAN}Binary:${NC}          ${SISY_BIN}
EOF
}

# ── Build ────────────────────────────────────────────────
build() {
    if [ -x "${SISY_BIN}" ] && [ "${FORCE_BUILD}" = false ]; then
        echo -e "${GREEN}✓${NC} Binary already exists: ${SISY_BIN}"
        return 0
    fi
    if [ "${FORCE_BUILD}" = true ]; then
        echo -e "${YELLOW}⟳${NC} Forcing rebuild..."
    else
        echo -e "${YELLOW}⟳${NC} Building sisyphus (release)..."
    fi
    cargo build --release -p sisyphus-cli --manifest-path "${SISY_ROOT}/Cargo.toml"
    echo -e "${GREEN}✓${NC} Build complete: ${SISY_BIN}"
}

# ── Bridge ───────────────────────────────────────────────
BRIDGE_PORT=50051
BRIDGE_PID=""

# Check if a TCP port is listening (portable, no root needed)
port_is_listening() {
    local port="$1"
    # Try ss first (fast, most Linux)
    if command -v ss &>/dev/null; then
        ss -tln 2>/dev/null | grep -q ":${port}[[:space:]]" && return 0
    fi
    # Fallback: nc (sometimes available)
    if command -v nc &>/dev/null; then
        nc -z -w1 ::1 "${port}" 2>/dev/null && return 0
    fi
    # Fallback: bash /dev/tcp
    timeout 1 bash -c "echo > /dev/tcp/[::1]/${port}" 2>/dev/null && return 0
    return 1
}

start_bridge() {
    local addr="http://[::1]:${BRIDGE_PORT}"
    if port_is_listening "${BRIDGE_PORT}"; then
        echo -e "${GREEN}✓${NC} Bridge already running at ${addr}"
        return 0
    fi
    echo -e "${YELLOW}⟳${NC} Starting bridge daemon..."
    local bridge_bin="${SISY_ROOT}/target/release/sisyphus-bridge"

    # 强制重建 bridge（如果 -f 指定）或二进制不存在时构建
    if [ "${FORCE_BUILD}" = true ] || [ ! -x "${bridge_bin}" ]; then
        echo -e "${YELLOW}⟳${NC} Building sisyphus-bridge..."
        cargo build --release -p sisyphus-bridge --manifest-path "${SISY_ROOT}/Cargo.toml"
    fi

    if [ ! -x "${bridge_bin}" ]; then
        echo -e "${RED}✗${NC} Bridge binary not found at ${bridge_bin}"
        return 1
    fi

    # 在项目目录下启动 bridge（config.toml 查找路径）
    SISYPHUS_CONFIG="${SISY_ROOT}/config.toml" \
    "${bridge_bin}" &
    BRIDGE_PID=$!

    # 等待 bridge 就绪（最多 15 秒）
    local waited=0
    while [ "${waited}" -lt 15 ]; do
        if port_is_listening "${BRIDGE_PORT}"; then
            echo -e "${GREEN}✓${NC} Bridge daemon ready (PID: ${BRIDGE_PID})"
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    echo -e "${YELLOW}⚠${NC} Bridge not confirmed ready (continuing anyway, PID: ${BRIDGE_PID})"
}

stop_bridge() {
    if [ -n "${BRIDGE_PID}" ]; then
        kill "${BRIDGE_PID}" 2>/dev/null || true
        wait "${BRIDGE_PID}" 2>/dev/null || true
        echo -e "${GREEN}✓${NC} Bridge daemon stopped"
    fi
}

stop_bridge() {
    if [ -n "${BRIDGE_PID}" ]; then
        kill "${BRIDGE_PID}" 2>/dev/null || true
        wait "${BRIDGE_PID}" 2>/dev/null || true
        echo -e "${GREEN}✓${NC} Bridge daemon stopped"
    fi
}

# ── Run ──────────────────────────────────────────────────
run() {
    if [ $# -eq 0 ]; then
        if [ "${WITH_BRIDGE}" = true ]; then
            start_bridge
            trap stop_bridge EXIT
        fi
        echo -e "${CYAN}⟳${NC} Launching TUI (Ctrl+C to exit)..."
        "${SISY_BIN}" -i --data-dir "${SISY_DATA}"
    else
        exec "${SISY_BIN}" --data-dir "${SISY_DATA}" "$@"
    fi
}

# ── Main ─────────────────────────────────────────────────
case "${1:-}" in
    help|--help|-h)
        usage
        ;;
    build|--build)
        build
        ;;
    *)
        build
        run "$@"
        ;;
esac
