use crate::build::{self, BuildOutput, Diag, LanguageBackend, TestResults};
use crate::{ToolError, ToolResult};
use std::path::Path;

/// Java backend supporting Maven and Gradle.
#[derive(Debug)]
pub struct JavaBackend {
    /// Build tool: "mvn" or "gradle"
    tool: String,
}

impl JavaBackend {
    pub fn new_maven() -> Self {
        Self { tool: "mvn".into() }
    }

    pub fn new_gradle() -> Self {
        Self {
            tool: "gradle".into(),
        }
    }
}

impl LanguageBackend for JavaBackend {
    fn name(&self) -> &str {
        if self.tool == "mvn" {
            "java-maven"
        } else {
            "java-gradle"
        }
    }

    fn check(&self, _pkg: Option<&str>, _all: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        let args = if self.tool == "mvn" {
            vec!["compile"]
        } else {
            vec!["compileJava"]
        };
        let (stdout, stderr, success) = build::run_command(&self.tool, &args, cwd)?;
        let (errors, warnings) = parse_mvn_output(&stderr, &stdout);
        Ok(BuildOutput {
            errors,
            warnings,
            raw_stdout: stdout,
            raw_stderr: stderr,
            success,
        })
    }

    fn build(
        &self,
        _pkg: Option<&str>,
        _release: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput> {
        let args = if self.tool == "mvn" {
            vec!["package"]
        } else {
            vec!["build"]
        };
        let (stdout, stderr, success) = build::run_command(&self.tool, &args, cwd)?;
        let (errors, warnings) = parse_mvn_output(&stderr, &stdout);
        Ok(BuildOutput {
            errors,
            warnings,
            raw_stdout: stdout,
            raw_stderr: stderr,
            success,
        })
    }

    fn test(
        &self,
        _pkg: Option<&str>,
        name: Option<&str>,
        _all: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<TestResults> {
        let args = if self.tool == "mvn" {
            let mut a = vec!["test"];
            if let Some(n) = name {
                a.push("-Dtest");
                a.push(n);
            }
            a
        } else {
            let mut a = vec!["test"];
            if let Some(n) = name {
                a.push("--tests");
                a.push(n);
            }
            a
        };
        let (stdout, stderr, _success) = build::run_command(&self.tool, &args, cwd)?;
        Ok(parse_mvn_test_results(&stdout, &stderr))
    }

    fn lint(&self, _pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        let args = if self.tool == "mvn" {
            vec!["checkstyle:check"]
        } else {
            vec!["check"]
        };
        let (stdout, stderr, success) = build::run_command(&self.tool, &args, cwd)?;
        let (errors, warnings) = parse_checkstyle_output(&stderr, &stdout);
        Ok(BuildOutput {
            errors,
            warnings,
            raw_stdout: stdout,
            raw_stderr: stderr,
            success,
        })
    }

    fn fix(
        &self,
        _pkg: Option<&str>,
        _apply_lint_fixes: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput> {
        // Java auto-fix is limited; try google-java-format if available
        if build::run_command("google-java-format", &["--version"], cwd).is_ok() {
            build::run_command(
                "google-java-format",
                &["--replace", "--skip-regex", "."],
                cwd,
            )?;
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
        if build::run_command("google-java-format", &["--version"], cwd).is_ok() {
            let (_stdout, stderr, success) =
                build::run_command("google-java-format", &["--dry-run", "."], cwd)?;
            // google-java-format exits non-zero if any file needs formatting
            return Ok(success && stderr.is_empty());
        }
        Ok(true)
    }

    fn fmt_write(&self, cwd: Option<&Path>) -> ToolResult<()> {
        if build::run_command("google-java-format", &["--version"], cwd).is_ok() {
            build::run_command(
                "google-java-format",
                &["--replace", "--skip-regex", "."],
                cwd,
            )?;
        }
        Ok(())
    }

    fn doc(&self, _pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<String> {
        let args = if self.tool == "mvn" {
            vec!["javadoc:javadoc"]
        } else {
            vec!["javadoc"]
        };
        let (stdout, stderr, success) = build::run_command(&self.tool, &args, cwd)?;
        let combined = format!("{stdout}\n{stderr}");
        if success {
            Ok(combined)
        } else {
            Err(ToolError::CommandFailed {
                code: 1,
                stdout,
                stderr,
            })
        }
    }

    fn run(&self, _args: &[&str], cwd: Option<&Path>) -> ToolResult<String> {
        let args = if self.tool == "mvn" {
            vec!["exec:java"]
        } else {
            vec!["run"]
        };
        let (stdout, stderr, success) = build::run_command(&self.tool, &args, cwd)?;
        let combined = format!("{stdout}{stderr}");
        if success {
            Ok(combined)
        } else {
            Err(ToolError::CommandFailed {
                code: 1,
                stdout,
                stderr,
            })
        }
    }
}

// ── Parsers ───────────────────────────────────────────

fn parse_mvn_output(stderr: &str, _stdout: &str) -> (Vec<Diag>, Vec<Diag>) {
    // Maven errors: [ERROR] /path/to/File.java:[10,5] message
    // or: [ERROR] message without location
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for line in stderr.lines() {
        let t = line.trim();
        if t.starts_with("[ERROR]") {
            let rest = t.trim_start_matches("[ERROR]").trim();
            // Try to parse location: /path/File.java:[line,col]
            if let Some(bracket_start) = rest.rfind(":[") {
                if let Some(bracket_end) = rest[bracket_start..].find(']') {
                    let abs_end = bracket_start + bracket_end;
                    let between = &rest[bracket_start + 2..abs_end]; // "line,col"
                    let file = rest[..bracket_start].trim().to_string();
                    let msg = rest[abs_end + 1..].trim().to_string();

                    if !file.is_empty()
                        && !file.starts_with("Failed to")
                        && !file.starts_with("Unable to")
                        && !file.starts_with("Could not")
                    {
                        let (line_no, col) = between
                            .split_once(',')
                            .map(|(l, c)| (l.trim().parse().ok(), c.trim().parse().ok()))
                            .unwrap_or((None, None));
                        errors.push(Diag {
                            file: Some(file),
                            line: line_no,
                            col,
                            level: "error".into(),
                            code: None,
                            message: msg,
                        });
                        continue;
                    }
                }
            }
            // No location: generic error
            errors.push(Diag {
                file: None,
                line: None,
                col: None,
                level: "error".into(),
                code: None,
                message: rest.to_string(),
            });
        } else if t.starts_with("[WARNING]") {
            let rest = t.trim_start_matches("[WARNING]").trim();
            warnings.push(Diag {
                file: None,
                line: None,
                col: None,
                level: "warning".into(),
                code: None,
                message: rest.to_string(),
            });
        }
    }

    (errors, warnings)
}

fn parse_checkstyle_output(_stderr: &str, stdout: &str) -> (Vec<Diag>, Vec<Diag>) {
    // Checkstyle output in Maven format usually goes to stdout
    // Example: /path/to/File.java:10:5: warning: message [rule]
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for line in stdout.lines() {
        let t = line.trim();
        if let Some(pos) = t.find(": warning: ") {
            let file_part = &t[..pos];
            let msg = t[pos + 11..].to_string();
            let parts: Vec<&str> = file_part.rsplitn(3, ':').collect();
            if parts.len() >= 3 {
                let col = parts[0].parse().ok();
                let line_no = parts[1].parse().ok();
                warnings.push(Diag {
                    file: Some(parts[2].to_string()),
                    line: line_no,
                    col,
                    level: "warning".into(),
                    code: None,
                    message: msg,
                });
            }
        } else if let Some(pos) = t.find(": error: ") {
            let file_part = &t[..pos];
            let msg = t[pos + 9..].to_string();
            let parts: Vec<&str> = file_part.rsplitn(3, ':').collect();
            if parts.len() >= 3 {
                let col = parts[0].parse().ok();
                let line_no = parts[1].parse().ok();
                errors.push(Diag {
                    file: Some(parts[2].to_string()),
                    line: line_no,
                    col,
                    level: "error".into(),
                    code: None,
                    message: msg,
                });
            }
        }
    }

    (errors, warnings)
}

fn parse_mvn_test_results(stdout: &str, stderr: &str) -> TestResults {
    // Maven test output:
    //   Tests run: 10, Failures: 1, Errors: 0, Skipped: 0
    //   [INFO] --- surefire:...
    //   Failed tests: com.example.MyTest#testMethod
    let combined = format!("{stdout}\n{stderr}");
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut failures = Vec::new();

    for line in combined.lines() {
        let t = line.trim();
        // Extract failure names
        if t.starts_with("Failed tests:") {
            let rest = t.trim_start_matches("Failed tests:").trim();
            // Each test name is on the same line or next lines
            for name in rest.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                failures.push(crate::build::TestFailure {
                    name: name.to_string(),
                });
            }
        }
    }

    // Parse summary: "Tests run: 10, Failures: 1, Errors: 0, Skipped: 0"
    for line in combined.lines() {
        let t = line.trim();
        if t.starts_with("Tests run:") {
            for part in t.split(',') {
                let p = part.trim();
                if let Some(n) = p
                    .strip_prefix("Tests run:")
                    .and_then(|s| s.trim().split(' ').next())
                    .and_then(|s| s.parse().ok())
                {
                    // Tests run includes both passed and failed; compute passed = total - failures - errors
                    let total: usize = n;
                    let fail_count: usize = combined
                        .lines()
                        .filter_map(|l| {
                            let l = l.trim();
                            if l.starts_with("Failures:") || l.starts_with("Errors:") {
                                l.split(',')
                                    .next()?
                                    .split(':')
                                    .nth(1)?
                                    .trim()
                                    .parse::<usize>()
                                    .ok()
                            } else {
                                None
                            }
                        })
                        .sum();
                    // Or just fall back to reasonable defaults
                    passed = total.saturating_sub(fail_count);
                }
                if let Some(n) = p
                    .strip_prefix("Failures:")
                    .and_then(|s| s.trim().split(' ').next())
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    failed += n;
                }
            }
        }
    }

    TestResults {
        passed,
        failed,
        ignored: 0,
        measured: 0,
        filtered_out: 0,
        failures,
        raw_stdout: stdout.to_string(),
        raw_stderr: stderr.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mvn_output_error() {
        let stderr =
            "[ERROR] /home/src/main/java/App.java:[10,5] some error\n[WARNING] some warning\n";
        let (errors, warnings) = parse_mvn_output(stderr, "");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].file.as_deref().unwrap_or("").contains("App.java"));
        assert_eq!(errors[0].line, Some(10));
        assert_eq!(errors[0].col, Some(5));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_backend_names() {
        let mvn = JavaBackend::new_maven();
        assert_eq!(mvn.name(), "java-maven");
        let gradle = JavaBackend::new_gradle();
        assert_eq!(gradle.name(), "java-gradle");
    }
}
