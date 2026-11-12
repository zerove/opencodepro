//! CLI 侧 `sisyphus dev` 的审批闸（T2.2 一致性）。
//!
//! TUI 的 dev 路径通过 `DevAskApprover` 走统一权限体系；**CLI 的 `dev` 此前
//! 完全没有 approver** —— 计划里的 `RunCommand`（任意 shell）直接执行，既不
//! 匹配规则、也不写审计、更不会询问。自动化入口（脚本 / CI / agent 调用）
//! 恰恰是最不该静默执行任意命令的地方。
//!
//! 这里补上与 TUI 相同的判定口径：
//!
//! - `Allow` → 放行（审计 `allowed`）；
//! - `Deny`  → 拒绝（审计 `denied`）—— 显式 deny 规则永远优先，`--yes` 也不放行；
//! - `Ask`   → 交互式终端问 `[Y/n]`；**非交互环境默认拒绝**（fail-safe），
//!   除非显式 `--yes`（自动化必须显式授权）。
//!
//! 审计记录沿用 `permission` 模块的同一套 resolved 标记
//! （`allowed` / `denied` / `asked-approved` / `asked-denied`）。

use std::io::{IsTerminal, Write};
use std::sync::Arc;

use anyhow::Result;

use crate::permission::{CliPermission, Decision};

/// CLI 侧 dev 审批器。
pub struct CliDevApprover {
    perms: Arc<CliPermission>,
    /// `--yes`：把 `Ask` 判定直接放行（显式 deny 规则仍然拒绝）。
    auto_approve: bool,
    /// stdin 是否为终端（决定能否交互询问）。
    interactive: bool,
}

impl CliDevApprover {
    /// 构造审批器。
    ///
    /// `interactive` 显式传入，便于测试（不依赖测试环境的 stdin）。
    pub fn new(perms: Arc<CliPermission>, auto_approve: bool, interactive: bool) -> Self {
        Self {
            perms,
            auto_approve,
            interactive,
        }
    }

    /// 按当前环境探测交互能力构造（非测试路径）。
    pub fn from_env(perms: Arc<CliPermission>, auto_approve: bool) -> Self {
        let interactive = std::io::stdin().is_terminal();
        Self::new(perms, auto_approve, interactive)
    }

    /// 权限判定（供测试与诊断使用）。
    pub fn permissions(&self) -> &CliPermission {
        &self.perms
    }

    /// 一步动作的可读描述（用于提示与审计）。
    fn detail_of(action: &serde_json::Value) -> String {
        let raw = action
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| action.to_string());
        if raw.chars().count() > 300 {
            let truncated: String = raw.chars().take(300).collect();
            format!("{truncated}…")
        } else {
            raw
        }
    }
}

#[async_trait::async_trait]
impl crate::tools::developer::DevApprover for CliDevApprover {
    async fn approve(&self, tool_id: &str, action: &serde_json::Value) -> Result<bool, String> {
        let decision = self.perms.evaluate(tool_id);
        match decision.decision {
            Decision::Allow => {
                self.perms.record_decision(tool_id, &decision, "allowed");
                Ok(true)
            },
            Decision::Deny => {
                self.perms.record_decision(tool_id, &decision, "denied");
                eprintln!("🚫 已拒绝（权限规则）：{tool_id}");
                Ok(false)
            },
            Decision::Ask => {
                let detail = Self::detail_of(action);
                if self.auto_approve {
                    eprintln!("⚠️  已自动放行（--yes）：{tool_id} — {detail}");
                    self.perms.record_decision(tool_id, &decision, "asked-approved");
                    return Ok(true);
                }
                if !self.interactive {
                    // fail-safe：非交互环境不能询问 → 拒绝。
                    eprintln!(
                        "🚫 非交互环境无法确认，已拒绝：{tool_id}\n    详情：{detail}\n    自动化场景请显式加 --yes（或在权限规则里 allow 该工具）。"
                    );
                    self.perms.record_decision(tool_id, &decision, "asked-denied");
                    return Ok(false);
                }
                eprint!("⚠  Allow {tool_id}? [Y/n] ");
                let _ = std::io::stderr().flush();
                let mut buf = String::new();
                match std::io::stdin().read_line(&mut buf) {
                    Err(_) => {
                        self.perms.record_decision(tool_id, &decision, "asked-unanswered");
                        Ok(false)
                    },
                    // `Ok(0)` = stdin EOF：没有应答，不能当成「回车同意」。
                    Ok(0) => {
                        self.perms.record_decision(tool_id, &decision, "asked-unanswered");
                        Ok(false)
                    },
                    Ok(_) => {
                        let answer = buf.trim().to_lowercase();
                        let approved = answer.is_empty() || answer == "y" || answer == "yes";
                        self.perms.record_decision(
                            tool_id,
                            &decision,
                            if approved {
                                "asked-approved"
                            } else {
                                "asked-denied"
                            },
                        );
                        Ok(approved)
                    },
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::{DecisionSource, PermissionDecision};
    use crate::tools::developer::DevApprover;
    use std::path::PathBuf;

    /// 通过**真实的权限配置文件**构造权限（顺带覆盖配置文件解析路径）。
    fn perms_with(rules: &[(&str, &str)], audit: PathBuf) -> CliPermission {
        let dir = audit.parent().expect("审计路径需带父目录");
        std::fs::create_dir_all(dir).expect("创建目录");
        let cfg = dir.join("permissions.json");
        let items: Vec<String> = rules
            .iter()
            .map(|(pattern, permission)| {
                format!(r#"{{"pattern": "{pattern}", "permission": "{permission}"}}"#)
            })
            .collect();
        std::fs::write(&cfg, format!("[{}]", items.join(","))).expect("写权限配置");
        CliPermission::load(Some(cfg)).expect("加载权限配置").with_audit_path(audit)
    }

    fn action_with_command(cmd: &str) -> serde_json::Value {
        serde_json::json!({ "type": "RunCommand", "params": { "command": cmd } })
    }

    #[tokio::test]
    async fn ask_is_denied_in_non_interactive_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let audit = dir.path().join("audit.jsonl");
        // 未命中规则 + 危险工具 → 默认 ask
        let perms = Arc::new(perms_with(&[], audit.clone()));
        let approver = CliDevApprover::new(perms, false, false);

        let allowed = approver
            .approve("builtin:bash_exec", &action_with_command("rm -rf /tmp/x"))
            .await
            .expect("审批不应出错");

        assert!(!allowed, "非交互环境必须 fail-safe 拒绝");
        let content = std::fs::read_to_string(&audit).expect("审计文件");
        assert!(
            content.contains("asked-denied"),
            "审计应记录拒绝: {content}"
        );
    }

    #[tokio::test]
    async fn auto_approve_allows_ask() {
        let dir = tempfile::tempdir().expect("tempdir");
        let audit = dir.path().join("audit.jsonl");
        let perms = Arc::new(perms_with(&[], audit.clone()));
        let approver = CliDevApprover::new(perms, true, false);

        let allowed = approver
            .approve("builtin:bash_exec", &action_with_command("cargo check"))
            .await
            .expect("审批不应出错");

        assert!(allowed, "--yes 应放行 ask 判定");
        let content = std::fs::read_to_string(&audit).expect("审计文件");
        assert!(
            content.contains("asked-approved"),
            "审计应记录放行: {content}"
        );
    }

    #[tokio::test]
    async fn explicit_deny_wins_even_with_auto_approve() {
        let dir = tempfile::tempdir().expect("tempdir");
        let audit = dir.path().join("audit.jsonl");
        let perms = Arc::new(perms_with(&[("builtin:bash_exec", "deny")], audit.clone()));
        let approver = CliDevApprover::new(perms, true, false);

        let allowed = approver
            .approve("builtin:bash_exec", &action_with_command("rm -rf /"))
            .await
            .expect("审批不应出错");

        assert!(!allowed, "显式 deny 规则必须优先于 --yes");
        let content = std::fs::read_to_string(&audit).expect("审计文件");
        assert!(content.contains("denied"), "审计应记录拒绝: {content}");
    }

    #[tokio::test]
    async fn allow_rule_passes_without_audit_of_ask() {
        let dir = tempfile::tempdir().expect("tempdir");
        let audit = dir.path().join("audit.jsonl");
        let perms = Arc::new(perms_with(&[("builtin:bash_exec", "allow")], audit.clone()));
        let approver = CliDevApprover::new(perms, false, false);

        let allowed = approver
            .approve("builtin:bash_exec", &action_with_command("cargo check"))
            .await
            .expect("审批不应出错");

        assert!(allowed, "allow 规则应直接放行");
        let content = std::fs::read_to_string(&audit).expect("审计文件");
        assert!(content.contains("allowed"), "审计应记录放行: {content}");
    }

    #[test]
    fn detail_is_truncated_for_long_commands() {
        let long = "x".repeat(500);
        let detail = CliDevApprover::detail_of(&action_with_command(&long));
        assert!(
            detail.chars().count() <= 301,
            "详情应被截断: {}",
            detail.len()
        );
        assert!(detail.ends_with('…'));
    }

    #[test]
    fn decision_source_is_reported() {
        // 审计口径与 permission 模块一致（规则 / 危险默认 / 默认）
        let decision = PermissionDecision {
            decision: Decision::Ask,
            source: DecisionSource::DangerousDefault,
        };
        assert_eq!(decision.source.as_audit_str(), "dangerous-default");
    }
}
