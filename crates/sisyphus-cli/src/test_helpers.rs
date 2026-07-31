//! Test helpers for parsing CLI commands.
//!
//! Subcommand enums derive `clap::Subcommand`, which does not provide
//! `try_parse_from` (that is `clap::Parser`-only). These helpers parse a
//! full command line through the top-level `Cli` (which derives `Parser`).

use crate::commands::Commands;
use crate::Cli;
use clap::Parser;

/// Parse a command chain from raw args (excluding the binary name).
///
/// Example: `parse_commands(&["build", "test", "-p", "sisyphus-core"])`
///
/// # Panics
/// Panics if the args fail to parse or if no command is provided.
pub fn parse_commands(args: &[&str]) -> Commands {
    let full: Vec<&str> = std::iter::once("sisyphus")
        .chain(args.iter().copied())
        .collect();
    let cli = Cli::try_parse_from(full).expect("failed to parse CLI args");
    cli.command.expect("no command provided")
}
