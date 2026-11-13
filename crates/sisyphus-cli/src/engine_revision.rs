//! 引擎版本归因（实现已迁移到 `sisyphus-metrics`）。
//!
//! 保留本模块作为 CLI 的稳定路径：`handle_dev` / `handle_verify` 与既有测试
//! 通过 `sisyphus_cli::engine_revision::engine_revision()` 取版本。bridge 的
//! 执行记录复用同一实现，避免两处各自探测 git 导致口径分叉。
//!
//! 语义说明见 `sisyphus_metrics::engine_revision` 模块文档。

pub use sisyphus_metrics::engine_revision::{engine_revision, engine_revision_at, EngineRevision};
