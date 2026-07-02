use crate::build::{self, BuildOutput, Diag, LanguageBackend, TestResults};
use sisyphus_tools_core::{ToolError, ToolResult};
use std::path::Path;

/// Python backend using `mypy` (check), `ruff` (lint/format), `pytest` (test).
#[derive(Debug)]
pub struct PythonBackend {
    /// Python command: "python3" or "python"
    python: String,
}

impl PythonBackend {
    pub fn new() -> ToolResult<Self> {
        for cmd in &["python3", "python"] {
            if build::run_command(cmd, &["--version"], None).is_ok() {
                return Ok(Self {
                    python: cmd.to_string(),
                });
            }
        }
        Err(ToolError::Generic(
            "No Python interpreter found (python3/python)".into(),
        ))
    }
}

impl LanguageBackend for PythonBackend {
    fn name(&self) -> &'static str {
        "python"
    }

    fn check(&self, _pkg: Option<&str>, _all: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        // Try mypy first, then pyright
        if build::run_command("mypy", &["--version"], cwd).is_ok() {
            let (stdout, stderr, success) = build::run_command("mypy", &["."], cwd)?;
            let (errors, warnings) = parse_mypy_output(&stderr);
            return Ok(BuildOutput {
                errors,
                warnings,
                raw_stdout: stdout,
                raw_stderr: stderr,
                success,
            });
        }
        // No type checker — pass
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
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput> {
        // Python doesn't have a compile step; install deps as build equivalent
        let has_pyproject = cwd
            .is_some_and(|d| d.join("pyproject.toml").exists());
        let (stdout, stderr, success) = if has_pyproject {
            // Try `pip install -e .` or just validate
            build::run_command(&self.python, &["-m", "pip", "install", "-e", "."], cwd)?
        } else {
            (String::new(), String::new(), true)
        };
        Ok(BuildOutput {
            errors: vec![],
            warnings: vec![],
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
        let mut args = vec!["-m", "pytest"];
        if let Some(n) = name {
            args.push("-k");
            args.push(n);
        }
        let (stdout, stderr, _success) = build::run_command(&self.python, &args, cwd)?;
        Ok(parse_pytest_results(&stdout, &stderr))
    }

    fn lint(&self, _pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        if build::run_command("ruff", &["--version"], cwd).is_ok() {
            let (stdout, stderr, success) = build::run_command("ruff", &["check", "."], cwd)?;
            let (errors, warnings) = parse_ruff_output(&stderr, &stdout);
            return Ok(BuildOutput {
                errors,
                warnings,
                raw_stdout: stdout,
                raw_stderr: stderr,
                success,
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

    fn fix(
        &self,
        _pkg: Option<&str>,
        apply_lint_fixes: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput> {
        if apply_lint_fixes && build::run_command("ruff", &["--version"], cwd).is_ok() {
            build::run_command("ruff", &["check", "--fix", "."], cwd)?;
        }
        // Also run ruff format
        if build::run_command("ruff", &["--version"], cwd).is_ok() {
            build::run_command("ruff", &["format", "."], cwd)?;
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
        if build::run_command("ruff", &["--version"], cwd).is_ok() {
            let (_stdout, _stderr, success) =
                build::run_command("ruff", &["format", "--check", "."], cwd)?;
            return Ok(success);
        }
        Ok(true)
    }

    fn fmt_write(&self, cwd: Option<&Path>) -> ToolResult<()> {
        if build::run_command("ruff", &["--version"], cwd).is_ok() {
            build::run_command("ruff", &["format", "."], cwd)?;
        }
        Ok(())
    }

    fn doc(&self, _pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<String> {
        if build::run_command("pdoc", &["--version"], cwd).is_ok() {
            let (stdout, stderr, success) = build::run_command("pdoc", &["."], cwd)?;
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
        } else {
            Ok("pdoc not installed — no docs generated".into())
        }
    }

    fn run(&self, _args: &[&str], cwd: Option<&Path>) -> ToolResult<String> {
        let entry = detect_entry_point(cwd);
        let (stdout, stderr, success) = build::run_command(&self.python, &[&entry], cwd)?;
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

fn detect_entry_point(cwd: Option<&Path>) -> String {
    let cwd = cwd.unwrap_or_else(|| std::path::Path::new("."));
    for name in &["main.py", "app.py", "manage.py"] {
        if cwd.join(name).exists() {
            return name.to_string();
        }
    }
    // Check pyproject.toml for [project.scripts]
    let pyproject = cwd.join("pyproject.toml");
    if pyproject.exists() {
        if let Ok(content) = std::fs::read_to_string(&pyproject) {
            if let Some(line) = content.lines().find(|l| l.contains('=') && l.contains(':')) {
                if let Some(cmd) = line.split('=').next() {
                    let cmd = cmd.trim().trim_matches('"').trim_matches('\'');
                    if !cmd.is_empty() {
                        return format!("-m {}", cmd.replace('-', "_"));
                    }
                }
            }
        }
    }
    "main.py".to_string()
}

// ── Parsers ───────────────────────────────────────────

fn parse_mypy_output(stderr: &str) -> (Vec<Diag>, Vec<Diag>) {
    // mypy format: file:line:col: error: message
    // or: file:line:col: warning: message
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for line in stderr.lines() {
        let t = line.trim();
        // Parse: filepath:line:col: level: message
        if let Some(pos) = t.find(": error: ") {
            let file_part = &t[..pos];
            let msg = t[pos + 9..].to_string(); // after ": error: "
            let (file, line_no, col) = parse_colon_location(file_part);
            errors.push(Diag {
                file,
                line: line_no,
                col,
                level: "error".into(),
                code: None,
                message: msg,
            });
        } else if let Some(pos) = t.find(": warning: ") {
            let file_part = &t[..pos];
            let msg = t[pos + 11..].to_string();
            let (file, line_no, col) = parse_colon_location(file_part);
            warnings.push(Diag {
                file,
                line: line_no,
                col,
                level: "warning".into(),
                code: None,
                message: msg,
            });
        }
    }

    (errors, warnings)
}

fn parse_ruff_output(stderr: &str, _stdout: &str) -> (Vec<Diag>, Vec<Diag>) {
    // ruff format: file:line:col: code message
    // Example: src/app.py:10:5: F401 `os` imported but unused
    let mut warnings = Vec::new();

    for line in stderr.lines() {
        let t = line.trim();
        // Ruff output lines match: file:line:col: CODE message
        // Count colons — diagnostic lines have at least 3 colons (file:line:col: CODE)
        let colons: Vec<usize> = t.match_indices(':').map(|(i, _)| i).collect();
        if colons.len() >= 3 {
            let file = Some(t[..colons[0]].to_string());
            let line_no = t[colons[0] + 1..colons[1]].trim().parse().ok();
            let col = t[colons[1] + 1..colons[2]].trim().parse().ok();
            let rest = t[colons[2] + 1..].trim();
            let code_end = rest.find(' ').unwrap_or(rest.len());
            let code = Some(rest[..code_end].to_string());
            let msg = rest[code_end..].trim().to_string();
            // ruff outputs are warnings by default
            warnings.push(Diag {
                file,
                line: line_no,
                col,
                level: "warning".into(),
                code,
                message: msg,
            });
        }
    }

    (vec![], warnings)
}

fn parse_pytest_results(stdout: &str, stderr: &str) -> TestResults {
    // pytest output:
    //   ========== short test summary info ==========
    //   PASSED src/test_a.py::test_func
    //   FAILED src/test_b.py::test_bad
    //   ========== 1 passed, 1 failed in 0.12s ==========
    let combined = format!("{stdout}\n{stderr}");
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut failures = Vec::new();

    for line in combined.lines() {
        let t = line.trim();
        if t.starts_with("PASSED ") {
            passed += 1;
        } else if t.starts_with("FAILED ") {
            failed += 1;
            let name = t.trim_start_matches("FAILED ").trim().to_string();
            if !name.is_empty() {
                failures.push(crate::build::TestFailure { name });
            }
        }
    }

    // Also try to parse summary: "= 1 passed, 2 failed in 0.5s ="
    for line in combined.lines() {
        let t = line.trim().trim_matches('=').trim();
        if t.contains("passed") || t.contains("failed") {
            for part in t.split(',') {
                let p = part.trim();
                if let Some(n) = p.strip_suffix(" passed") {
                    passed = passed.max(n.trim().parse().unwrap_or(0));
                } else if let Some(n) = p.strip_suffix(" failed") {
                    failed = failed.max(n.trim().parse().unwrap_or(0));
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

fn parse_colon_location(s: &str) -> (Option<String>, Option<usize>, Option<usize>) {
    // Format: "file:line:col"
    let parts: Vec<&str> = s.rsplitn(3, ':').collect();
    if parts.len() >= 3 {
        let col = parts[0].parse().ok();
        let line = parts[1].parse().ok();
        let file = Some(parts[2].to_string());
        (file, line, col)
    } else {
        (Some(s.to_string()), None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mypy_output() {
        let stderr = "src/app.py:10:5: error: Incompatible return value type\nsrc/utils.py:1:1: warning: Unused import\n";
        let (errors, warnings) = parse_mypy_output(stderr);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file.as_deref(), Some("src/app.py"));
        assert_eq!(errors[0].line, Some(10));
        assert_eq!(errors[0].col, Some(5));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_parse_pytest_results() {
        let stdout = "\n= short test summary info =\nPASSED tests/test_a.py::test_func\nFAILED tests/test_b.py::test_bad\n= 1 passed, 1 failed in 0.12s =\n";
        let r = parse_pytest_results(stdout, "");
        // pytest summary overrides per-line counts
        assert!(r.passed >= 1);
        assert!(r.failed >= 1);
    }

    #[test]
    fn test_parse_colon_location() {
        let (file, line, col) = parse_colon_location("src/app.py:10:5");
        assert_eq!(file.as_deref(), Some("src/app.py"));
        assert_eq!(line, Some(10));
        assert_eq!(col, Some(5));
    }
}
