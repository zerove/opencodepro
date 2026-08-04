use anyhow::{Context, Result};
use std::path::PathBuf;

/// 从配置文件加载规则的简单 CLI 权限存储。
#[derive(Debug, Default)]
pub struct CliPermission {
    rules: Vec<(String, crate::tools::Permission)>,
}

impl CliPermission {
    /// 从 JSON 配置文件加载权限规则。
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let config_path = path.unwrap_or_else(|| {
            let mut p = PathBuf::from(".");
            p.push(".sisyphus-permissions.json");
            p
        });

        let mut this = Self { rules: Vec::new() };

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .context("Failed to read permission config")?;
            let raw: Vec<RawRule> =
                serde_json::from_str(&content).context("Failed to parse permission config")?;
            for r in raw {
                let perm = match r.permission.to_lowercase().as_str() {
                    "allow" => crate::tools::Permission::Allow,
                    "deny" => crate::tools::Permission::Deny,
                    "ask" => crate::tools::Permission::Ask,
                    other => anyhow::bail!("Unknown permission value: {other}"),
                };
                this.rules.push((r.pattern, perm));
            }
        }

        Ok(this)
    }

    /// 检查工具是否被允许。允许时返回 `Some(true)`，拒绝时返回 `Some(false)`，
    /// 需要询问时返回 `None`。
    pub fn check(&self, tool_name: &str) -> Option<bool> {
        for (pattern, perm) in &self.rules {
            if tool_name == pattern {
                return match perm {
                    crate::tools::Permission::Allow => Some(true),
                    crate::tools::Permission::Deny => Some(false),
                    crate::tools::Permission::Ask => None,
                };
            }
        }
        // 默认：允许
        Some(true)
    }
}

#[derive(serde::Deserialize)]
struct RawRule {
    pattern: String,
    permission: String,
}
