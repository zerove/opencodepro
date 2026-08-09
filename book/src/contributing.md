# 贡献指南

## 开发环境设置

1. **前置条件**：Rust 1.96+、Cargo
2. **克隆**：`git clone https://github.com/zeronhq/sisyphus-wasm`
3. **构建**：`cargo build --workspace`
4. **测试**：`cargo test --workspace`

## 代码质量

CI 流水线强制：

- **格式化**：`cargo fmt --all --check`
- **静态检查**：`cargo clippy --workspace --all-targets`（零警告）
- **测试**：`cargo test --workspace`（全部通过）
- **供应链**：`cargo deny check`（许可证、禁用项、安全公告）

## 项目结构

```
crates/
  sisyphus-types/    — 核心数据结构（无依赖）
  sisyphus-metrics/  — Prometheus 可观测性
  sisyphus-sandbox/  — WASM 沙箱运行时
  sisyphus-intel/    — 认知与模拟
  sisyphus-mem/      — 多级缓存
  sisyphus-core/     — 内核与编排
  sisyphus-bridge/   — 入口点与 API
book/                — mdBook 文档
```

## 指南

- **生产代码中禁止 `unwrap()`** — 使用 `expect("描述性消息")` 或通过 `?` 传播。
- **无 `as any` 或 `@ts-ignore` 等价物** — 保持类型安全。
- **测试覆盖** — 为新功能添加测试，尤其是边界情况。
- **指标** — 通过 `sisyphus_metrics` 为重要操作埋点。

## WASM 工具开发

工具 WASM 二进制放在 workspace 根目录的 `wasm_tools/`。每个二进制应导出接受并返回字节数组的 `run` 函数。