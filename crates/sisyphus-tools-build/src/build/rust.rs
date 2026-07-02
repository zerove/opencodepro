use crate::build::{self, BuildOutput, Diag, LanguageBackend, TestFailure, TestResults};
use sisyphus_tools_core::{ToolError, ToolResult};
use std::path::Path;

/// Rust build system backend using `cargo`.
#[derive(Debug, Clone, Copy)]
pub struct RustBackend;

impl LanguageBackend for RustBackend {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn check(&self, pkg: Option<&str>, all: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        let mut args = vec!["check"];
        if let Some(p) = pkg {
            args.push("--package");
            args.push(p);
        }
        if all {
            args.push("--workspace");
        }
        let (stdout, stderr, success) = build::cargo_raw(&args, cwd)?;
        let (errors, warnings) = extract_diags(&stderr);
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
        pkg: Option<&str>,
        release: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput> {
        let mut args = vec!["build"];
        if release {
            args.push("--release");
        }
        if let Some(p) = pkg {
            args.push("--package");
            args.push(p);
        }
        let (stdout, stderr, success) = build::cargo_raw(&args, cwd)?;
        let (errors, warnings) = extract_diags(&stderr);
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
        pkg: Option<&str>,
        name: Option<&str>,
        all: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<TestResults> {
        let mut args = vec!["test"];
        if let Some(n) = name {
            args.push("--");
            args.push(n);
        }
        if let Some(p) = pkg {
            args.push("--package");
            args.push(p);
        }
        if all {
            args.push("--workspace");
        }
        let (stdout, stderr, _success) = build::cargo_raw(&args, cwd)?;
        Ok(parse_test_results(&stdout, &stderr))
    }

    fn lint(&self, pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        let mut args = vec!["clippy"];
        if let Some(p) = pkg {
            args.push("--package");
            args.push(p);
        }
        let (stdout, stderr, success) = build::cargo_raw(&args, cwd)?;
        let (errors, warnings) = extract_diags(&stderr);
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
        pkg: Option<&str>,
        apply_lint_fixes: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput> {
        let mut args = vec!["fix"];
        if apply_lint_fixes {
            args.push("--clippy");
        }
        if let Some(p) = pkg {
            args.push("--package");
            args.push(p);
        }
        let (stdout, stderr, success) = build::cargo_raw(&args, cwd)?;
        let (errors, warnings) = extract_diags(&stderr);
        Ok(BuildOutput {
            errors,
            warnings,
            raw_stdout: stdout,
            raw_stderr: stderr,
            success,
        })
    }

    fn fmt_check(&self, cwd: Option<&Path>) -> ToolResult<bool> {
        let (_stdout, _stderr, success) = build::cargo_raw(&["fmt", "--check"], cwd)?;
        Ok(success)
    }

    fn fmt_write(&self, cwd: Option<&Path>) -> ToolResult<()> {
        build::cargo_raw(&["fmt"], cwd)?;
        Ok(())
    }

    fn doc(&self, pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<String> {
        let mut args = vec!["doc", "--no-deps"];
        if let Some(p) = pkg {
            args.push("--package");
            args.push(p);
        }
        let (stdout, stderr, success) = build::cargo_raw(&args, cwd)?;
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

    fn run(&self, args: &[&str], cwd: Option<&Path>) -> ToolResult<String> {
        let mut cargo_args = vec!["run"];
        cargo_args.extend_from_slice(args);
        // If no extra args, just `cargo run`
        let (stdout, stderr, success) = build::cargo_raw(&cargo_args, cwd)?;
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

// ── Diagnostic parser for Rust (cargo/clippy output) ──

pub fn extract_diags(stderr: &str) -> (Vec<Diag>, Vec<Diag>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut lines = stderr.lines().peekable();

    while let Some(line) = lines.next() {
        let (level, code, msg): (String, Option<String>, String) =
            if let Some(rest) = line.strip_prefix("error[") {
                let code_end = rest.find(']').unwrap_or(0);
                let code = rest[..code_end].to_string();
                let msg = rest[code_end + 1..]
                    .trim()
                    .trim_start_matches(':')
                    .trim()
                    .to_string();
                (String::from("error"), Some(code), msg)
            } else if let Some(rest) = line.strip_prefix("error:") {
                if rest.trim().starts_with("aborting due to")
                    || rest.trim().starts_with("could not compile")
                    || rest
                        .trim()
                        .contains("For more information about this error")
                {
                    continue;
                }
                (String::from("error"), None, rest.trim().to_string())
            } else if let Some(rest) = line.strip_prefix("warning[") {
                let code_end = rest.find(']').unwrap_or(0);
                let code = rest[..code_end].to_string();
                let msg = rest[code_end + 1..]
                    .trim()
                    .trim_start_matches(':')
                    .trim()
                    .to_string();
                (String::from("warning"), Some(code), msg)
            } else if let Some(rest) = line.strip_prefix("warning:") {
                (String::from("warning"), None, rest.trim().to_string())
            } else {
                continue;
            };

        let (file, ln, col) = if let Some(loc_line) = lines.peek() {
            let loc = loc_line.trim();
            if let Some(loc) = loc.strip_prefix("--> ") {
                let loc = loc.trim();
                let mut parts = loc.rsplitn(3, ':');
                let col_str = parts.next().and_then(|s| s.parse().ok());
                let line_str = parts.next().and_then(|s| s.parse().ok());
                let f = parts.next().map(std::string::ToString::to_string);
                lines.next();
                (f, line_str, col_str)
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
        };

        let is_error = level == "error";
        let diag = Diag {
            file,
            line: ln,
            col,
            level,
            code,
            message: msg,
        };
        if is_error {
            errors.push(diag);
        } else {
            warnings.push(diag);
        }
    }

    (errors, warnings)
}

// ── Test results parser (cargo test output) ───────────

pub fn parse_test_results(stdout: &str, stderr: &str) -> TestResults {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut ignored = 0usize;
    let mut measured = 0usize;
    let mut filtered_out = 0usize;
    let mut failures = Vec::new();

    let combined = format!("{stdout}\n{stderr}");
    for line in combined.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("test result: ") {
            let body = if let Some(b) = trimmed.strip_prefix("test result: ") {
                if let Some(b) = b.strip_prefix("ok.") {
                    b.trim()
                } else if let Some(b) = b.strip_prefix("FAILED.") {
                    b.trim()
                } else {
                    b
                }
            } else {
                continue;
            };
            for segment in body.split(';') {
                let s = segment.trim();
                if let Some(n) = s.strip_suffix(" passed") {
                    passed += n.trim().parse().unwrap_or(0);
                } else if let Some(n) = s.strip_suffix(" failed") {
                    failed += n.trim().parse().unwrap_or(0);
                } else if let Some(n) = s.strip_suffix(" ignored") {
                    ignored += n.trim().parse().unwrap_or(0);
                } else if let Some(n) = s.strip_suffix(" measured") {
                    measured += n.trim().parse().unwrap_or(0);
                } else if let Some(n) = s.strip_suffix(" filtered out") {
                    filtered_out += n.trim().parse().unwrap_or(0);
                }
            }
        }
    }

    let mut in_failures = false;
    for line in combined.lines() {
        let trimmed = line.trim();
        if trimmed == "failures:" {
            in_failures = true;
            continue;
        }
        if in_failures {
            if trimmed.is_empty() || trimmed.starts_with("test result") {
                in_failures = false;
            } else if !trimmed.starts_with("----") && !trimmed.starts_with("FAILED") {
                failures.push(TestFailure {
                    name: trimmed.to_string(),
                });
            }
        }
    }

    TestResults {
        passed,
        failed,
        ignored,
        measured,
        filtered_out,
        failures,
        raw_stdout: stdout.to_string(),
        raw_stderr: stderr.to_string(),
    }
}

impl RustBackend {
    /// Access the raw diagnostic parser (for reuse in diagnose.rs)
    pub fn parse_diags(stderr: &str) -> (Vec<Diag>, Vec<Diag>) {
        extract_diags(stderr)
    }

    /// Access the raw test results parser
    pub fn parse_test_results(stdout: &str, stderr: &str) -> TestResults {
        parse_test_results(stdout, stderr)
    }
}

// ── Tests ─────────────────────────────────────────────

#[cfg(test)]
#[path = "rust_tests.rs"]
mod tests;
