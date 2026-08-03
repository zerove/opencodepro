//! gRPC 处理器子模块。
//!
//! - `execution`：核心执行域（SOP 执行、目标分解、工具执行、流式执行、认知委托）
//! - `manage`：管理域（代理、工具、技能、插件、工作区、会话）
//! - `misc`：辅助域（健康检查、反馈、经验、基准、熔断、混沌工程）

pub(crate) mod execution;
pub(crate) mod manage;
pub(crate) mod misc;
