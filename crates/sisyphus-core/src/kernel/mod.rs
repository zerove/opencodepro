//! 核心内核实现 — `SisyphusKernel` 的主要方法。
//!
//! 为可读性从 `lib.rs` 中拆出。通过 `pub use kernel::*` 重新导出。
//! impl 拆分到各领域子模块；此处的 `pub use` 重新导出保持
//! 下游 crate 使用的原始模块路径稳定不变。

mod cache;
mod construction;
mod execution;
mod feedback;
mod project;
