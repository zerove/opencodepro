use serde::{Deserialize, Serialize};
use sisyphus_tools_core::ToolResult;

/// A security vulnerability found by dependency scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vulnerability {
    pub package: String,
    pub severity: String,
    pub advisory_url: String,
    pub description: String,
}

/// An outdated dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutdatedDep {
    pub name: String,
    pub current_version: String,
    pub latest_version: String,
}

/// Comprehensive dependency report.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DepReport {
    pub vulnerabilities: Vec<Vulnerability>,
    pub outdated: Vec<OutdatedDep>,
    pub dep_count: u64,
}

/// Manages project dependencies: vulnerability scanning, version updates, audits.
pub struct DependencyManager {
    manifest_path: String,
}

impl DependencyManager {
    /// Create a dependency manager for the given Cargo.toml path.
    pub fn new(manifest_path: &str) -> Self {
        Self {
            manifest_path: manifest_path.to_string(),
        }
    }

    /// Run cargo-deny to check for known vulnerabilities.
    ///
    /// Requires cargo-deny to be installed. Returns parsed results.
    pub async fn check_vulnerabilities(&self) -> ToolResult<Vec<Vulnerability>> {
        let output = tokio::process::Command::new("cargo")
            .args([
                "deny",
                "--manifest-path",
                &self.manifest_path,
                "check",
                "advisories",
                "--format=json",
            ])
            .output()
            .await
            .map_err(|e| {
                sisyphus_tools_core::ToolError::Generic(format!("Failed to run cargo-deny: {e}"))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() && !stderr.is_empty() {
            // cargo-deny may output advisories to stderr
            return Ok(Self::parse_deny_advisories(&stderr));
        }

        Ok(Self::parse_deny_advisories(&stdout))
    }

    /// Run cargo-outdated to find outdated dependencies.
    ///
    /// Requires cargo-outdated to be installed.
    pub async fn list_outdated(&self) -> ToolResult<Vec<OutdatedDep>> {
        let output = tokio::process::Command::new("cargo")
            .args([
                "outdated",
                "--manifest-path",
                &self.manifest_path,
                "--format=json",
            ])
            .output()
            .await
            .map_err(|e| {
                sisyphus_tools_core::ToolError::Generic(format!(
                    "Failed to run cargo-outdated: {e}"
                ))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(Self::parse_outdated(&stdout))
    }

    /// Update a dependency to a specific version in Cargo.toml.
    ///
    /// This is a best-effort text replacement. For complex version specs,
    /// manual review is recommended.
    pub fn update_dep(&self, name: &str, version: &str) -> ToolResult<String> {
        let content = std::fs::read_to_string(&self.manifest_path)
            .map_err(sisyphus_tools_core::ToolError::Io)?;

        // Try to find the dependency in the TOML file
        // Pattern: name = "version" or name = { version = "...", ... }
        let updated = content.clone();

        // Pattern 1: name = "X.Y.Z"
        let simple_pattern = format!("{name} = \"");
        if let Some(pos) = updated.find(&simple_pattern) {
            let val_start = pos + simple_pattern.len();
            if let Some(end) = updated[val_start..].find('\"') {
                let old_ver = updated[val_start..val_start + end].to_string();
                let mut updated = updated;
                updated.replace_range(val_start..val_start + end, version);
                let new_path = self.save_updated(&updated)?;
                return Ok(format!(
                    "Updated {name}: {old_ver} -> {version} in {new_path}"
                ));
            }
        }

        // Pattern 2: name = { version = "X.Y.Z", ... }
        let brace_pattern = format!("{name} = {{");
        if let Some(pos) = updated.find(&brace_pattern) {
            let rest = &updated[pos..];
            if let Some(ver_pos) = rest.find("version = \"") {
                let val_start = pos + ver_pos + "version = \"".len();
                if let Some(end) = updated[val_start..].find('\"') {
                    let old_ver = updated[val_start..val_start + end].to_string();
                    let mut updated = updated;
                    updated.replace_range(val_start..val_start + end, version);
                    let new_path = self.save_updated(&updated)?;
                    return Ok(format!(
                        "Updated {name}: {old_ver} -> {version} in {new_path}"
                    ));
                }
            }
        }

        Err(sisyphus_tools_core::ToolError::Generic(format!(
            "Could not find dependency '{}' in {}",
            name, self.manifest_path
        )))
    }

    /// Run a comprehensive check: vulnerabilities + outdated + dep count.
    pub async fn check_all(&self) -> ToolResult<DepReport> {
        let vulnerabilities = self.check_vulnerabilities().await.unwrap_or_default();
        let outdated = self.list_outdated().await.unwrap_or_default();
        let dep_count = self.count_dependencies();

        Ok(DepReport {
            vulnerabilities,
            outdated,
            dep_count: dep_count as u64,
        })
    }

    /// Count dependencies in Cargo.toml (rough estimate via line scan).
    pub fn count_dependencies(&self) -> usize {
        let content = std::fs::read_to_string(&self.manifest_path).ok();
        match content {
            Some(text) => text
                .lines()
                .filter(|l| {
                    let t = l.trim();
                    // Dependency lines look like: `name = "version"` or `name = { ... }`
                    !t.is_empty()
                        && !t.starts_with('#')
                        && !t.starts_with('[')
                        && (t.contains(" = \"") || t.contains("= {"))
                        && !t.starts_with("name")
                        && !t.starts_with("version")
                        && !t.starts_with("edition")
                })
                .count(),
            None => 0,
        }
    }

    fn save_updated(&self, content: &str) -> ToolResult<String> {
        std::fs::write(&self.manifest_path, content).map_err(sisyphus_tools_core::ToolError::Io)?;
        Ok(self.manifest_path.clone())
    }

    fn parse_deny_advisories(output: &str) -> Vec<Vulnerability> {
        // Try JSON parsing first
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output) {
            if let Some(advisories) = parsed.get("advisories").and_then(|a| a.as_array()) {
                return advisories
                    .iter()
                    .filter_map(|a| {
                        Some(Vulnerability {
                            package: a.get("package").and_then(|p| p.as_str())?.to_string(),
                            severity: a
                                .get("severity")
                                .and_then(|s| s.as_str())
                                .unwrap_or("unknown")
                                .to_string(),
                            advisory_url: a
                                .get("advisory")
                                .and_then(|a| a.get("url"))
                                .and_then(|u| u.as_str())
                                .unwrap_or("")
                                .to_string(),
                            description: a
                                .get("advisory")
                                .and_then(|a| a.get("description"))
                                .and_then(|d| d.as_str())
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect();
            }
        }

        // Fallback: line-based parsing for human-readable output
        let mut vulnerabilities = Vec::new();
        for line in output.lines() {
            if line.contains("ADV") && line.contains("└─") {
                let parts: Vec<&str> = line.split("└─").collect();
                if parts.len() > 1 {
                    vulnerabilities.push(Vulnerability {
                        package: "unknown".into(),
                        severity: "unknown".into(),
                        advisory_url: String::new(),
                        description: parts[1].trim().to_string(),
                    });
                }
            }
        }
        vulnerabilities
    }

    fn parse_outdated(output: &str) -> Vec<OutdatedDep> {
        // Try JSON format (cargo-outdated --format=json)
        if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(output) {
            return parsed
                .iter()
                .filter_map(|entry| {
                    Some(OutdatedDep {
                        name: entry.get("name").and_then(|n| n.as_str())?.to_string(),
                        current_version: entry
                            .get("project")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?")
                            .to_string(),
                        latest_version: entry
                            .get("latest")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?")
                            .to_string(),
                    })
                })
                .collect();
        }

        // Fallback: line-based for human-readable table
        let mut outdated = Vec::new();
        for line in output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[0] != "Name" && parts[0] != "##" {
                outdated.push(OutdatedDep {
                    name: parts[0].to_string(),
                    current_version: parts.get(1).unwrap_or(&"?").to_string(),
                    latest_version: parts.get(2).unwrap_or(&"?").to_string(),
                });
            }
        }
        outdated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_cargo_toml(content: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cargo.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, "{}", content).unwrap();
        (dir, path.to_string_lossy().to_string())
    }

    #[test]
    fn test_update_dep_simple_version() {
        let toml = r#"
[package]
name = "test"

[dependencies]
serde = "1.0"
tokio = "1.0"
"#;
        let (_dir, path) = create_temp_cargo_toml(toml);
        let dm = DependencyManager::new(&path);
        let result = dm.update_dep("serde", "2.0").unwrap();
        assert!(result.contains("serde"));
        assert!(result.contains("1.0"));
        assert!(result.contains("2.0"));

        // Verify file content
        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains(r#"serde = "2.0""#));
        assert!(updated.contains(r#"tokio = "1.0""#));
    }

    #[test]
    fn test_update_dep_brace_version() {
        let toml = r#"
[package]
name = "test"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
"#;
        let (_dir, path) = create_temp_cargo_toml(toml);
        let dm = DependencyManager::new(&path);
        let result = dm.update_dep("serde", "2.0").unwrap();
        assert!(result.contains("serde"));

        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains(r#"version = "2.0""#));
    }

    #[test]
    fn test_update_dep_not_found() {
        let toml = "[dependencies]\nserde = \"1.0\"\n";
        let (_dir, path) = create_temp_cargo_toml(toml);
        let dm = DependencyManager::new(&path);
        let result = dm.update_dep("nonexistent", "1.0");
        assert!(result.is_err());
    }

    #[test]
    fn test_count_dependencies() {
        let toml = r#"
[package]
name = "test"

[dependencies]
serde = "1.0"
tokio = { version = "1.0", features = ["full"] }

[dev-dependencies]
tempfile = "3"
"#;
        let (_dir, path) = create_temp_cargo_toml(toml);
        let dm = DependencyManager::new(&path);
        // At least 3 deps (serde, tokio, tempfile)
        assert!(dm.count_dependencies() >= 3);
    }

    #[test]
    fn test_parse_outdated_empty() {
        let parsed = DependencyManager::parse_outdated("");
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_parse_outdated_json() {
        let json = r#"[
  {"name": "serde", "project": "1.0.0", "latest": "1.0.200"},
  {"name": "tokio", "project": "1.0.0", "latest": "1.40.0"}
]"#;
        let parsed = DependencyManager::parse_outdated(json);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "serde");
        assert_eq!(parsed[0].latest_version, "1.0.200");
    }

    #[test]
    fn test_parse_deny_advisories_empty() {
        let vulns = DependencyManager::parse_deny_advisories("");
        assert!(vulns.is_empty());
    }

    #[test]
    fn test_dep_report_default() {
        let report = DepReport::default();
        assert!(report.vulnerabilities.is_empty());
        assert!(report.outdated.is_empty());
        assert_eq!(report.dep_count, 0);
    }
}
