use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::permission::CliPermission;

/// `crate::tools::bash::execute` 的包装，带权限检查
/// 与 `timeout_ms` 默认值。
pub async fn exec(
    command: &str,
    workdir: Option<&Path>,
    timeout_ms: u64,
    perms: &CliPermission,
) -> Result<crate::tools::BashOutput> {
    // 权限检查
    let decision = perms.evaluate("exec");
    match decision.decision {
        crate::permission::Decision::Allow => {
            perms.record_decision("exec", &decision, "allowed");
        },
        crate::permission::Decision::Deny => {
            perms.record_decision("exec", &decision, "denied");
            anyhow::bail!("Permission denied: shell execution is blocked by config");
        },
        crate::permission::Decision::Ask => {
            // TUI 模式：跳过权限提示 — 用户输入命令并回车
            // 已表明意图。同步的 stdin 读取与 crossterm 原始模式
            // 不兼容。
            // CLI 模式在 Commands::Exec 中有自己的权限检查。
            perms.record_decision("exec", &decision, "asked-proceeded-tui");
        },
    }

    let output = crate::tools::bash::execute(command, workdir, timeout_ms)
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

/// 将 `Duration` 格式化为人类可读字符串（例如 "1.23s"）。
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
