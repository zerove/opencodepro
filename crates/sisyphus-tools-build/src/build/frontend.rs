use crate::build::{self, BuildOutput, Diag, LanguageBackend, TestResults};
use sisyphus_tools_core::ToolResult;
use std::path::Path;

/// Frontend (HTML/CSS) backend using `stylelint` and `prettier`.
#[derive(Debug)]
pub struct FrontendBackend;

impl LanguageBackend for FrontendBackend {
    fn name(&self) -> &'static str {
        "frontend"
    }

    fn check(&self, _pkg: Option<&str>, _all: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        let mut all_errors = Vec::new();
        let mut all_warnings = Vec::new();

        // Run stylelint on CSS files if available
        if build::run_command("stylelint", &["--version"], cwd).is_ok() {
            let (stdout, stderr, success) =
                build::run_command("stylelint", &["**/*.css", "--formatter", "json"], cwd)?;
            let (errors, warnings) = parse_stylelint_json(&stdout);
            all_errors.extend(errors);
            all_warnings.extend(warnings);
            // If stylelint failed and we have errors, propagate success
            let overall_success = success && all_errors.is_empty();
            return Ok(BuildOutput {
                errors: all_errors,
                warnings: all_warnings,
                raw_stdout: stdout,
                raw_stderr: stderr,
                success: overall_success,
            });
        }

        Ok(BuildOutput {
            errors: vec![],
            warnings: vec![],
            raw_stdout: String::new(),
            raw_stderr: String::new(),
            success: true,
        })
    }

    fn build(
        &self,
        _pkg: Option<&str>,
        _release: bool,
        _cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput> {
        // HTML/CSS has no build step
        Ok(BuildOutput {
            errors: vec![],
            warnings: vec![],
            raw_stdout: String::new(),
            raw_stderr: String::new(),
            success: true,
        })
    }

    fn test(
        &self,
        _pkg: Option<&str>,
        _name: Option<&str>,
        _all: bool,
        _cwd: Option<&Path>,
    ) -> ToolResult<TestResults> {
        // HTML/CSS has no test step
        Ok(TestResults {
            passed: 0,
            failed: 0,
            ignored: 0,
            measured: 0,
            filtered_out: 0,
            failures: vec![],
            raw_stdout: String::new(),
            raw_stderr: String::new(),
        })
    }

    fn lint(&self, _pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        self.check(_pkg, false, cwd)
    }

    fn fix(
        &self,
        _pkg: Option<&str>,
        _apply_lint_fixes: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput> {
        // Run prettier if available
        if build::run_command("prettier", &["--version"], cwd).is_ok() {
            build::run_command("prettier", &["--write", "**/*.{css,html}"], cwd)?;
        }
        // Run stylelint --fix if available
        if build::run_command("stylelint", &["--version"], cwd).is_ok() {
            build::run_command("stylelint", &["--fix", "**/*.css"], cwd)?;
        }
        Ok(BuildOutput {
            errors: vec![],
            warnings: vec![],
            raw_stdout: String::new(),
            raw_stderr: String::new(),
            success: true,
        })
    }

    fn fmt_check(&self, cwd: Option<&Path>) -> ToolResult<bool> {
        if build::run_command("prettier", &["--version"], cwd).is_ok() {
            let (_stdout, _stderr, success) =
                build::run_command("prettier", &["--check", "**/*.{css,html}"], cwd)?;
            return Ok(success);
        }
        Ok(true)
    }

    fn fmt_write(&self, cwd: Option<&Path>) -> ToolResult<()> {
        if build::run_command("prettier", &["--version"], cwd).is_ok() {
            build::run_command("prettier", &["--write", "**/*.{css,html}"], cwd)?;
        }
        Ok(())
    }

    fn doc(&self, _pkg: Option<&str>, _cwd: Option<&Path>) -> ToolResult<String> {
        Ok("No documentation generator for HTML/CSS".into())
    }
}

// ── Parsers ───────────────────────────────────────────

fn parse_stylelint_json(stdout: &str) -> (Vec<Diag>, Vec<Diag>) {
    // stylelint JSON output (--formatter json):
    // [{
    //   "source": "path/to/file.css",
    //   "warnings": [{"line": 10, "column": 5, "severity": "error", "text": "message", "rule": "rule-name"}]
    // }]
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if let Ok(results) = serde_json::from_str::<Vec<serde_json::Value>>(stdout) {
        for result in results {
            let file = result["source"].as_str().map(std::string::ToString::to_string);
            if let Some(warns) = result["warnings"].as_array() {
                for w in warns {
                    let line = w["line"].as_u64().map(|n| n as usize);
                    let col = w["column"].as_u64().map(|n| n as usize);
                    let severity = w["severity"].as_str().unwrap_or("warning");
                    let text = w["text"].as_str().unwrap_or("");
                    let rule = w["rule"].as_str().map(std::string::ToString::to_string);
                    // stylelint text format: "message (rule)" — remove rule from text
                    let message = if let Some(rule_start) = text.rfind(" (") {
                        text[..rule_start].to_string()
                    } else {
                        text.to_string()
                    };

                    let diag = Diag {
                        file: file.clone(),
                        line,
                        col,
                        level: severity.into(),
                        code: rule,
                        message,
                    };
                    if severity == "error" {
                        errors.push(diag);
                    } else {
                        warnings.push(diag);
                    }
                }
            }
        }
    }

    (errors, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stylelint_json() {
        let json = r#"[{
            "source": "src/style.css",
            "warnings": [
                {"line": 10, "column": 5, "severity": "error", "text": "Unexpected !important (declaration-no-important)", "rule": "declaration-no-important"},
                {"line": 1, "column": 1, "severity": "warning", "text": "Unknown property (unknown-property)", "rule": "unknown-property"}
            ]
        }]"#;
        let (errors, warnings) = parse_stylelint_json(json);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file.as_deref(), Some("src/style.css"));
        assert_eq!(errors[0].line, Some(10));
        assert_eq!(errors[0].col, Some(5));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_backend_name() {
        let backend = FrontendBackend;
        assert_eq!(backend.name(), "frontend");
    }
}
