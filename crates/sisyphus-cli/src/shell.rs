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
            // Prompt the user on stdin/stdout before proceeding
            eprint!("⚠  Allow shell execution? [Y/n] ");
            use std::io::Write;
            std::io::stderr().flush().ok();
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).ok();
            let answer = buf.trim().to_lowercase();
            if !answer.is_empty() && answer != "y" && answer != "yes" {
                anyhow::bail!("Shell execution denied by user");
            }
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
