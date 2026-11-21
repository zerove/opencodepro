#!/usr/bin/env bash
# ==============================================================================
# Sisyphus 一键构建与引导安装脚本
# 用法: ./scripts/install.sh [--prefix /path/to/install] [--skip-build]
# ==============================================================================
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
CONFIG_DIR="$HOME/.sisyphus"
SKIP_BUILD="false"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --prefix)
            PREFIX="$2"
            BIN_DIR="$PREFIX/bin"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD="true"
            shift
            ;;
        *)
            echo "未知参数: $1"
            exit 1
            ;;
    esac
done

echo "==> [1/4] 检查环境与 Rust 工具链..."
if ! command -v cargo &> /dev/null; then
    echo "错误: 未检测到 cargo，请先安装 Rust (https://rustup.rs/)"
    exit 1
fi
cargo --version

echo "==> [2/4] 构建 Sisyphus 核心组件..."
if [ "$SKIP_BUILD" = "false" ]; then
    echo "正在以 release 模式构建 sisyphus-cli, sisyphus-bridge, sisyphus-sandboxd..."
    cargo build --release -p sisyphus-cli -p sisyphus-bridge -p sisyphus-sandboxd
else
    echo "跳过编译 (--skip-build)"
fi

echo "==> [3/4] 安装二进制文件到 $BIN_DIR..."
mkdir -p "$BIN_DIR"
mkdir -p "$CONFIG_DIR"

if [ -f "target/release/sisyphus" ]; then
    cp target/release/sisyphus "$BIN_DIR/sisyphus"
    chmod +x "$BIN_DIR/sisyphus"
    echo "已安装: $BIN_DIR/sisyphus"
else
    echo "警告: 未找到 target/release/sisyphus，请确认是否已成功构建。"
fi

if [ -f "target/release/sisyphus-bridge" ]; then
    cp target/release/sisyphus-bridge "$BIN_DIR/sisyphus-bridge"
    chmod +x "$BIN_DIR/sisyphus-bridge"
    echo "已安装: $BIN_DIR/sisyphus-bridge"
fi

if [ -f "target/release/sisyphus-sandboxd" ]; then
    cp target/release/sisyphus-sandboxd "$BIN_DIR/sisyphus-sandboxd"
    chmod +x "$BIN_DIR/sisyphus-sandboxd"
    echo "已安装: $BIN_DIR/sisyphus-sandboxd"
fi

echo "==> [4/4] 初始化默认配置文件..."
if [ ! -f "$CONFIG_DIR/config.toml" ]; then
    if [ -f "config.example.toml" ]; then
        cp config.example.toml "$CONFIG_DIR/config.toml"
        echo "已复制默认配置至 $CONFIG_DIR/config.toml"
    fi
fi

echo ""
echo "=============================================================="
echo "✨ Sisyphus 安装成功！"
echo "=============================================================="
echo "二进制路径: $BIN_DIR"
echo "配置文件路径: $CONFIG_DIR/config.toml"
echo ""
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
  echo "提示: 请确保将 $BIN_DIR 添加到您的 PATH 环境变量中："
  echo "  export PATH=\"\$PATH:$BIN_DIR\""
  echo ""
fi
echo "快速开始："
echo "  1. 启动桥接服务: sisyphus-bridge"
echo "  2. 启动 TUI 客户端: sisyphus"
echo "=============================================================="
