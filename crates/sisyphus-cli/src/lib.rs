#![allow(clippy::cast_precision_loss)]
//! Sisyphus CLI — Sisyphus 认知引擎的命令行界面。
//!
//! 该库 crate 暴露全部 CLI 模块以便测试。
//! `main.rs` 二进制是委托到这里的薄入口。

pub mod bridge;
pub mod build;
pub mod delegate;
pub mod diagnose;
pub mod git;
pub mod hints;
pub mod init;
pub mod lsp;
pub mod mcp;
pub mod permission;
pub mod platform;
pub mod platform_command;
pub mod plugin;
pub mod post_edit;
pub mod precommit;
pub mod remote;
pub mod run;
pub mod shell;
pub mod shell_compat;
pub mod skill;
pub mod tools;
pub mod tui;
