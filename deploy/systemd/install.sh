#!/usr/bin/env bash
# ── Sisyphus Bridge 安装脚本 (systemd) ─────────────────────────────
# 自动化部署：构建 → 安装二进制 → 配置 systemd → 启动服务
# 使用：
#   ./deploy/systemd/install.sh
#   sudo ./deploy/systemd/install.sh          # 安装到 /etc/systemd/system
#
# 前提：需要 root 权限（systemctl + 创建系统用户 + 写入 /etc）

set -euo pipefail

# 路径常量
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SERVICE_NAME="sisyphus-bridge"
SERVICE_USER="sisyphus"
BIN_DIR="/usr/local/bin"
CONFIG_DIR="/etc/sisyphus"
DATA_DIR="/var/lib/sisyphus"
LOG_DIR="/var/log/sisyphus"
SERVICE_FILE_SRC="$SCRIPT_DIR/${SERVICE_NAME}.service"
SERVICE_FILE_DST="/etc/systemd/system/${SERVICE_NAME}.service"

# 颜色
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_err()  { echo -e "${RED}[ERROR]${NC} $*" >&2; }

require_root() {
    if [[ $EUID -ne 0 ]]; then
        log_err "请以 root 权限运行：sudo $0"
        exit 1
    fi
}

build_binary() {
    log_info "编译 sisyphus-bridge（release 模式）..."
    cd "$REPO_ROOT"
    cargo build --release -p sisyphus-bridge
    if [[ ! -f target/release/sisyphus-bridge ]]; then
        log_err "编译失败：找不到 target/release/sisyphus-bridge"
        exit 1
    fi
    log_info "编译成功"
}

install_binary() {
    log_info "安装二进制到 $BIN_DIR ..."
    install -m 0755 target/release/sisyphus-bridge "$BIN_DIR/sisyphus-bridge"
    log_info "已安装：$BIN_DIR/sisyphus-bridge"
}

setup_directories() {
    log_info "创建数据/日志目录..."
    install -d -m 0755 -o "$SERVICE_USER" -g "$SERVICE_USER" "$DATA_DIR" 2>/dev/null || {
        log_warn "用户 $SERVICE_USER 不存在，先创建"
        useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
        install -d -m 0755 -o "$SERVICE_USER" -g "$SERVICE_USER" "$DATA_DIR"
    }
    install -d -m 0755 -o "$SERVICE_USER" -g "$SERVICE_USER" "$LOG_DIR"
}

install_config() {
    if [[ -f "$CONFIG_DIR/config.toml" ]]; then
        log_warn "$CONFIG_DIR/config.toml 已存在，跳过覆盖"
    else
        log_info "安装默认配置到 $CONFIG_DIR/config.toml ..."
        install -d -m 0755 "$CONFIG_DIR"
        install -m 0644 "$REPO_ROOT/config.example.toml" "$CONFIG_DIR/config.toml"
        log_warn "请编辑 $CONFIG_DIR/config.toml 设置 auth_token 等敏感字段"
    fi
}

install_service() {
    log_info "安装 systemd service ..."
    install -m 0644 "$SERVICE_FILE_SRC" "$SERVICE_FILE_DST"
    systemctl daemon-reload
    log_info "已安装：$SERVICE_FILE_DST"
}

enable_and_start() {
    log_info "启用并启动服务..."
    systemctl enable "$SERVICE_NAME"
    systemctl restart "$SERVICE_NAME"
    sleep 2
    if systemctl is-active --quiet "$SERVICE_NAME"; then
        log_info "服务已启动"
        log_info "查看状态：systemctl status $SERVICE_NAME"
        log_info "查看日志：journalctl -u $SERVICE_NAME -f"
    else
        log_err "服务启动失败"
        log_err "诊断：journalctl -xeu $SERVICE_NAME --no-pager"
        exit 1
    fi
}

verify_health() {
    local http_addr="${SISYPHUS_HTTP:-http://127.0.0.1:8080}"
    log_info "验证健康度端点 $http_addr/health/deep ..."
    if curl -sf --max-time 5 "$http_addr/health/deep" >/dev/null 2>&1; then
        log_info "健康度检查通过"
    else
        log_warn "健康度端点未响应（服务可能在启动中）"
    fi
}

main() {
    require_root
    build_binary
    install_binary
    setup_directories
    install_config
    install_service
    enable_and_start
    verify_health
    log_info "✓ 部署完成"
}

main "$@"
