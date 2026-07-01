#![allow(clippy::cast_precision_loss)]
//! Sisyphus CLI — command-line interface for the Sisyphus cognitive engine.
//!
//! This library crate exposes all CLI modules for testing.
//! The `main.rs` binary is a thin entry point that delegates here.

pub mod bridge;
pub mod build;
pub mod delegate;
pub mod diagnose;
pub mod git;
pub mod init;
pub mod lsp;
pub mod mcp;
pub mod permission;
pub mod platform;
pub mod platform_command;
pub mod plugin;
pub mod precommit;
pub mod remote;
pub mod run;
pub mod shell;
pub mod shell_compat;
pub mod skill;
pub mod tui;
