//! `sisyphus permission`：权限判定检查与审计日志查看。

use anyhow::Result;

use crate::commands::PermissionCommands;
use sisyphus_cli::permission::{read_audit, CliPermission};

/// 分发权限子命令。
pub fn handle_permission(sub: &PermissionCommands, perms: &CliPermission) -> Result<()> {
    match sub {
        PermissionCommands::Check { tool, json } => {
            let decision = perms.evaluate(tool);
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "tool": tool,
                        "decision": decision.decision.as_str(),
                        "source": decision.source.as_audit_str(),
                    }))?
                );
            } else {
                println!("{}", format_decision(tool, &decision));
            }
            Ok(())
        },
        PermissionCommands::Audit { limit, json } => {
            let path = perms.audit_path();
            let entries = read_audit(&path, *limit)
                .map_err(|e| anyhow::anyhow!("读取审计日志 {} 失败: {e}", path.display()))?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else if entries.is_empty() {
                println!("No audit entries at {}", path.display());
            } else {
                for entry in &entries {
                    println!(
                        "{} {} {} → {} ({})",
                        entry.ts, entry.tool, entry.decision, entry.resolved, entry.source
                    );
                }
            }
            Ok(())
        },
    }
}

fn format_decision(tool: &str, decision: &sisyphus_cli::permission::PermissionDecision) -> String {
    format!(
        "{tool}: {} (source: {})",
        decision.decision.as_str(),
        decision.source.as_audit_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_cli::permission::{Decision, DecisionSource, PermissionDecision};

    #[test]
    fn format_decision_shows_source() {
        let decision = PermissionDecision {
            decision: Decision::Ask,
            source: DecisionSource::DangerousDefault,
        };
        let line = format_decision("exec", &decision);
        assert_eq!(line, "exec: ask (source: dangerous-default)");
    }

    #[test]
    fn format_decision_shows_glob_rule() {
        let decision = PermissionDecision {
            decision: Decision::Deny,
            source: DecisionSource::Rule("builtin:*".to_string()),
        };
        assert_eq!(
            format_decision("builtin:file_read", &decision),
            "builtin:file_read: deny (source: rule:builtin:*)"
        );
    }
}
