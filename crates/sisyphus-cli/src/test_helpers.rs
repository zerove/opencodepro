//! 解析 CLI 命令的测试辅助。
//!
//! 子命令枚举派生 `clap::Subcommand`，它不提供
//! `try_parse_from`（仅 `clap::Parser` 有）。这些辅助通过顶层
//! `Cli`（派生 `Parser`）解析完整命令行。

use crate::commands::Commands;
use crate::Cli;
use clap::Parser;

/// 从原始参数解析命令链（不含二进制名）。
///
/// 示例：`parse_commands(&["build", "test", "-p", "sisyphus-core"])`
///
/// # 恐慌（Panics）
/// 参数解析失败或未提供命令时恐慌。
pub fn parse_commands(args: &[&str]) -> Commands {
    let full: Vec<&str> = std::iter::once("sisyphus")
        .chain(args.iter().copied())
        .collect();
    let cli = Cli::try_parse_from(full).expect("failed to parse CLI args");
    cli.command.expect("no command provided")
}
