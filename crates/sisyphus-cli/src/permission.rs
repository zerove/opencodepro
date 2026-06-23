use anyhow::{Context, Result};
use std::path::PathBuf;

/// Simple CLI permission store that loads rules from a config file.
#[derive(Debug, Default)]
pub struct CliPermission {
    rules: Vec<(String, sisyphus_tools::Permission)>,
}

impl CliPermission {
    /// Load permission rules from a JSON config file.
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let config_path = path.unwrap_or_else(|| {
            let mut p = PathBuf::from(".");
            p.push(".sisyphus-permissions.json");
            p
        });

        let mut this = Self {
            rules: Vec::new(),
        };

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .context("Failed to read permission config")?;
            let raw: Vec<RawRule> =
                serde_json::from_str(&content).context("Failed to parse permission config")?;
            for r in raw {
                let perm = match r.permission.to_lowercase().as_str() {
                    "allow" => sisyphus_tools::Permission::Allow,
                    "deny" => sisyphus_tools::Permission::Deny,
                    "ask" => sisyphus_tools::Permission::Ask,
                    other => anyhow::bail!("Unknown permission value: {other}"),
                };
                this.rules.push((r.pattern, perm));
            }
        }

        Ok(this)
    }

    /// Check if a tool is allowed. Returns `Some(true)` if allowed, `Some(false)` if denied,
    /// `None` if needs to ask.
    pub fn check(&self, tool_name: &str) -> Option<bool> {
        for (pattern, perm) in &self.rules {
            if tool_name.contains(pattern) {
                return match perm {
                    sisyphus_tools::Permission::Allow => Some(true),
                    sisyphus_tools::Permission::Deny => Some(false),
                    sisyphus_tools::Permission::Ask => None,
                };
            }
        }
        // Default: allow
        Some(true)
    }
}

#[derive(serde::Deserialize)]
struct RawRule {
    pattern: String,
    permission: String,
}
