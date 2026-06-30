use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::permission::CliPermission;

/// Wrapper around `sisyphus_tools::bash::execute` with permission checking
/// and a `timeout_ms` default.
pub async fn exec(
    command: &str,
    workdir: Option<&Path>,
    timeout_ms: u64,
    perms: &CliPermission,
) -> Result<sisyphus_tools::BashOutput> {
    // Permission check
    match perms.check("exec") {
        Some(true) => {} // allowed
        Some(false) => {
            anyhow::bail!("Permission denied: shell execution is blocked by config");
        }
        None => {
            // TUI mode: skip permission prompt — user already showed intent
            // by typing the command and pressing Enter. The synchronous
            // stdin read is incompatible with crossterm raw mode.
            // CLI mode has its own permission check in Commands::Exec.
        }
    }

    let output = sisyphus_tools::bash::execute(command, workdir, timeout_ms)
        .await
        .context("Shell execution failed")?;

    if output.exit_code != 0 {
        anyhow::bail!(
            "Command failed (exit={}): {}{}",
            output.exit_code,
            output.stderr,
            if output.stderr_truncated {
                " [stderr truncated]"
            } else {
                ""
            }
        );
    }

    Ok(output)
}

/// Format a `Duration` to a human-readable string (e.g., "1.23s").
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 1.0 {
        format!("{:.0}ms", secs * 1000.0)
    } else if secs < 60.0 {
        format!("{:.2}s", secs)
    } else {
        format!("{:.0}m {:.0}s", secs / 60.0, secs % 60.0)
    }
}
