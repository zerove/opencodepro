//! Sisyphus Bridge — gRPC 与 REST API 桥接服务。
//!
//! 提供 17 个 gRPC RPC 与 34 个 REST 端点（OpenAPI 3.1），
//! 支持认知执行、技能管理、工作区隔离、混沌工程等能力。

#![recursion_limit = "256"]
pub mod auth;
pub mod auth_jwt;
pub mod auth_middleware;
pub mod bridge_service;
pub mod consensus;
pub mod grpc;
pub mod grpc_exec;
pub mod grpc_handlers;
pub mod http;
pub mod http_openapi;
pub mod jcode_caller;
pub mod sandboxd_client;
pub mod strategic_executor;
pub mod tools;

pub use bridge_service::MySisyphusBridge;
