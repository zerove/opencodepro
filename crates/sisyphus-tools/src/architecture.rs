use crate::ToolResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Result of analyzing a single module's architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleReport {
    pub name: String,
    pub file_path: String,
    pub public_fns: Vec<String>,
    pub public_types: Vec<String>,
    /// Modules this module imports from.
    pub imports: Vec<String>,
    /// Estimated coupling score (number of external imports / total imports).
    pub coupling_score: f64,
}

/// A suggested refactoring target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactoringSuggestion {
    pub module: String,
    pub issue: String,
    pub severity: String, // "high" | "medium" | "low"
    pub suggestion: String,
    pub estimated_effort: String, // "small" | "medium" | "large"
}

/// An auto-generated ADR-like document for a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureDoc {
    pub module_name: String,
    pub reports: Vec<ModuleReport>,
    pub coupling_analysis: String,
    pub suggestions: Vec<RefactoringSuggestion>,
    pub generated_at: u64,
}

/// Analyzes Rust source code to produce architecture reports.
pub struct ArchitectureAnalyzer;

impl ArchitectureAnalyzer {
    /// Produce a report for a single .rs file by scanning imports and public items.
    ///
    /// This is a line-based scan that looks for:
    /// - `pub fn` / `pub struct` / `pub enum` / `pub trait` declarations
    /// - `use ...` import statements
    pub fn analyze_file(file_path: &str) -> ToolResult<ModuleReport> {
        let content = std::fs::read_to_string(file_path)
            .map_err(crate::ToolError::Io)?;
        let name = Path::new(file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut public_fns = Vec::new();
        let mut public_types = Vec::new();
        let mut imports = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Detect public functions
            if let Some(rest) = trimmed.strip_prefix("pub fn ") {
                let fn_name = rest.split('(').next().unwrap_or("").to_string();
                if !fn_name.is_empty() {
                    public_fns.push(fn_name);
                }
            }

            // Detect public types
            for kw in &["pub struct ", "pub enum ", "pub trait ", "pub type ", "pub union "] {
                if let Some(rest) = trimmed.strip_prefix(kw) {
                    let type_name = rest.split('<')
                        .next()
                        .unwrap_or("")
                        .split('{')
                        .next()
                        .unwrap_or("")
                        .split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if !type_name.is_empty() && !public_types.contains(&type_name) {
                        public_types.push(type_name);
                    }
                }
            }

            // Detect imports from other crates/modules
            if let Some(path) = trimmed.strip_prefix("use ") {
                let import_path = path
                    .trim_end_matches(';')
                    .trim()
                    .to_string();
                if !import_path.is_empty() && !import_path.starts_with("crate::") {
                    let crate_name = import_path.split("::").next().unwrap_or("").to_string();
                    if !crate_name.is_empty() && !imports.contains(&crate_name) {
                        imports.push(crate_name);
                    }
                }
            }
        }

        let total_imports = imports.len() as f64;
        // Coupling = external imports / (public items + 1)  -- higher means more dependent on others
        let pub_count = (public_fns.len() + public_types.len()) as f64;
        let coupling_score = if pub_count > 0.0 {
            (total_imports / pub_count).min(10.0)
        } else {
            total_imports.min(10.0)
        };

        Ok(ModuleReport {
            name,
            file_path: file_path.to_string(),
            public_fns,
            public_types,
            imports,
            coupling_score,
        })
    }

    /// Analyze all .rs files in a directory and generate an architecture doc.
    pub fn analyze_directory(dir: &str) -> ToolResult<ArchitectureDoc> {
        let module_name = Path::new(dir)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut reports = Vec::new();
        let mut all_suggestions = Vec::new();

        let entries = std::fs::read_dir(dir)
            .map_err(crate::ToolError::Io)?;

        for entry in entries {
            let entry = entry.map_err(crate::ToolError::Io)?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                if let Ok(report) = Self::analyze_file(&path.to_string_lossy()) {
                    // Generate suggestions for high-coupling modules
                    if report.coupling_score > 5.0 {
                        all_suggestions.push(RefactoringSuggestion {
                            module: report.name.clone(),
                            issue: format!("High coupling ({:.2})", report.coupling_score),
                            severity: "high".into(),
                            suggestion: format!(
                                "Consider splitting {} into smaller modules or extracting shared logic into a common crate",
                                report.name
                            ),
                            estimated_effort: "medium".into(),
                        });
                    } else if report.coupling_score > 3.0 {
                        all_suggestions.push(RefactoringSuggestion {
                            module: report.name.clone(),
                            issue: format!("Moderate coupling ({:.2})", report.coupling_score),
                            severity: "medium".into(),
                            suggestion: format!(
                                "Review imports in {} for potential interface segregation",
                                report.name
                            ),
                            estimated_effort: "small".into(),
                        });
                    }
                    // Empty modules with lots of imports
                    if report.public_fns.is_empty() && report.public_types.is_empty() && report.imports.len() > 3 {
                        all_suggestions.push(RefactoringSuggestion {
                            module: report.name.clone(),
                            issue: "Module has imports but no public API".into(),
                            severity: "low".into(),
                            suggestion: "Add public exports or merge into a parent module".into(),
                            estimated_effort: "small".into(),
                        });
                    }
                    reports.push(report);
                }
            }
        }

        let coupling_analysis = if reports.is_empty() {
            "No Rust source files found".to_string()
        } else {
            let avg_coupling: f64 = reports.iter().map(|r| r.coupling_score).sum::<f64>() / reports.len() as f64;
            let high_coupling_count = reports.iter().filter(|r| r.coupling_score > 5.0).count();
            format!(
                "Analyzed {} modules. Average coupling score: {:.2}. {} modules with high coupling (>{:.0}). {} suggestions generated.",
                reports.len(),
                avg_coupling,
                high_coupling_count,
                5.0,
                all_suggestions.len()
            )
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(ArchitectureDoc {
            module_name,
            reports,
            coupling_analysis,
            suggestions: all_suggestions,
            generated_at: now,
        })
    }

    /// Generate a markdown ADR-style document from the analysis.
    pub fn generate_adr(doc: &ArchitectureDoc) -> String {
        let mut md = String::new();
        md.push_str(&format!("# Architecture Analysis: {}\n\n", doc.module_name));
        md.push_str(&format!("_Generated at UNIX timestamp {}_\n\n", doc.generated_at));
        md.push_str("## Summary\n\n");
        md.push_str(&doc.coupling_analysis);
        md.push_str("\n\n");

        if !doc.suggestions.is_empty() {
            md.push_str("## Refactoring Suggestions\n\n");
            for s in &doc.suggestions {
                md.push_str(&format!(
                    "- **{}** [{}] [{}]: {}\n  - Suggestion: {}\n\n",
                    s.module, s.severity, s.estimated_effort, s.issue, s.suggestion
                ));
            }
        }

        md.push_str("## Module Details\n\n");
        for report in &doc.reports {
            md.push_str(&format!(
                "### {}\n\n- File: `{}`\n- Public functions: {}\n- Public types: {}\n- External imports: {}\n- Coupling score: {:.2}\n\n",
                report.name,
                report.file_path,
                report.public_fns.len(),
                report.public_types.len(),
                report.imports.len(),
                report.coupling_score,
            ));
        }

        md
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_rs(content: &str) -> (tempfile::NamedTempFile, String) {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{}", content).unwrap();
        let path = f.path().to_string_lossy().to_string();
        (f, path)
    }

    #[test]
    fn test_analyze_file_detects_public_fns() {
        let code = r#"
use std::collections::HashMap;
use serde::Serialize;

pub fn hello() -> String {
    "world".to_string()
}

fn private() {}

pub struct MyStruct {
    pub field: i32,
}
"#;
        let (_f, path) = create_temp_rs(code);
        let report = ArchitectureAnalyzer::analyze_file(&path).unwrap();
        assert!(report.public_fns.contains(&"hello".to_string()));
        assert!(report.public_types.contains(&"MyStruct".to_string()));
        assert!(report.imports.contains(&"std".to_string()));
        assert!(report.imports.contains(&"serde".to_string()));
    }

    #[test]
    fn test_analyze_file_empty() {
        let code = "// just a comment\nfn nothing() {}\n";
        let (_f, path) = create_temp_rs(code);
        let report = ArchitectureAnalyzer::analyze_file(&path).unwrap();
        assert!(report.public_fns.is_empty());
        assert!(report.public_types.is_empty());
        assert!(report.imports.is_empty());
        assert_eq!(report.coupling_score, 0.0);
    }

    #[test]
    fn test_high_coupling_detected() {
        let code = r#"
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use tokio::sync::Mutex;
use reqwest::Client;
use tracing::info;
use anyhow::Result;

pub fn do_thing() -> Result<()> {
    Ok(())
}
"#;
        let (_f, path) = create_temp_rs(code);
        let report = ArchitectureAnalyzer::analyze_file(&path).unwrap();
        // 6 imports / 1 pub fn = 6.0 coupling
        assert!(report.coupling_score > 5.0);
    }

    #[test]
    fn test_analyze_directory_nonexistent() {
        let result = ArchitectureAnalyzer::analyze_directory("/tmp/nonexistent_dir_xyz123");
        assert!(result.is_err());
    }

    #[test]
    fn test_adr_generation() {
        let doc = ArchitectureDoc {
            module_name: "test".into(),
            reports: vec![],
            coupling_analysis: "No issues".into(),
            suggestions: vec![RefactoringSuggestion {
                module: "foo".into(),
                issue: "test issue".into(),
                severity: "high".into(),
                suggestion: "refactor".into(),
                estimated_effort: "small".into(),
            }],
            generated_at: 0,
        };
        let adr = ArchitectureAnalyzer::generate_adr(&doc);
        assert!(adr.contains("Architecture Analysis: test"));
        assert!(adr.contains("test issue"));
        assert!(adr.contains("refactor"));
    }
}
