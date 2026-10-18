//! Sisyphus Bridge — gRPC 与 REST API 桥接服务。
//!
//! 提供 17 个 gRPC RPC 与 34 个 REST 端点（OpenAPI 3.1），
//! 支持认知执行、技能管理、工作区隔离、混沌工程等能力。

// tonic gRPC handler 的返回类型被框架强制为 `Result<_, tonic::Status>`，
// `tonic::Status` 的 Err 变体较大（>=176 字节）触发 clippy::result_large_err。
// 签名与框架耦合无法修改，与 sisyphus-bridge-api 生成代码保持一致，整体允许。
#![allow(clippy::result_large_err)]
// `double_must_use`：`#[async_trait]` 会给 trait 的 async 方法加 `#[must_use]`，
// 而其返回类型（boxed Future）本身已带 `#[must_use]`，clippy 1.99.0 起判为冗余。
// consensus 等模块的 trait 经由该宏生成，无法逐个方法挂 allow。
#![allow(clippy::double_must_use)]
#![recursion_limit = "256"]
pub mod auth;
pub mod auth_jwt;
pub mod bridge_service;
pub mod consensus;
pub mod grpc;
pub mod grpc_exec;
pub mod grpc_handlers;
pub mod http;
pub mod http_openapi;
pub mod jcode_caller;
pub mod peer;
pub mod sandboxd_client;
pub mod strategic_executor;
pub mod tools;

pub use bridge_service::MySisyphusBridge;
