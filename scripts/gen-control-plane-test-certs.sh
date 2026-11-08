#!/usr/bin/env bash
# ADR-019：生成控制面 mTLS 测试 PKI（委托 Rust/rcgen 实现，无需 openssl）。
#
# 用法：
#   scripts/gen-control-plane-test-certs.sh [--out DIR] [--node NAME ...]
#   make gen-control-plane-test-certs ARGS="--out ./control-plane-certs --node node-1 --node node-2"
#
# 生成 CA + 节点证书（serverAuth+clientAuth 双用途）；节点 SAN 含
# `DNS:<node>,DNS:localhost,IP:127.0.0.1`。仅测试用途，禁止用于生产信任链。

set -euo pipefail

MANIFEST="$(cd "$(dirname "$0")/.." && pwd)/crates/sisyphus-bridge/Cargo.toml"
exec cargo run --manifest-path "$MANIFEST" --no-default-features \
  --example gen_control_plane_test_certs -- "$@"
