use crate::build::{self, BuildOutput, Diag, LanguageBackend, TestResults};
use sisyphus_tools_core::{ToolError, ToolResult};
use std::path::Path;

/// Go backend using `go` toolchain.
#[derive(Debug, Clone, Copy)]
pub struct GoBackend;

impl LanguageBackend for GoBackend {
    fn name(&self) -> &'static str {
        "go"
    }

    fn check(&self, _pkg: Option<&str>, _all: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        let (stdout, stderr, success) = build::run_command("go", &["vet", "./..."], cwd)?;
        let (errors, warnings) = parse_go_vet_output(&stderr, &stdout);
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
        let (stdout, stderr, success) = build::run_command("go", &["build", "./..."], cwd)?;
        let (errors, warnings) = parse_go_build_output(&stderr);
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
        let mut args = vec!["test", "./...", "-v"];
        if let Some(n) = name {
            args.push("-run");
            args.push(n);
        }
        let (stdout, stderr, _success) = build::run_command("go", &args, cwd)?;
        Ok(parse_go_test_results(&stdout, &stderr))
    }

    fn lint(&self, _pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        // Try staticcheck if available, otherwise fall back to go vet
        if build::run_command("staticcheck", &["--version"], cwd).is_ok() {
            let (stdout, stderr, success) = build::run_command("staticcheck", &["./..."], cwd)?;
            let (errors, warnings) = parse_staticcheck_output(&stderr, &stdout);
            return Ok(BuildOutput {
                errors,
                warnings,
                raw_stdout: stdout,
                raw_stderr: stderr,
                success,
            });
        }
        // Fallback: go vet as lint
        build::run_command("go", &["vet", "./..."], cwd).map(|(stdout, stderr, success)| {
            let (errors, warnings) = parse_go_vet_output(&stderr, &stdout);
            BuildOutput {
                errors,
                warnings,
                raw_stdout: stdout,
                raw_stderr: stderr,
                success,
            }
        })
    }

    fn fix(
        &self,
        _pkg: Option<&str>,
        apply_lint_fixes: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput> {
        // gofmt -s -w simplifies code; go fix applies API migrations
        build::run_command("gofmt", &["-s", "-w", "."], cwd)?;
        if apply_lint_fixes {
            build::run_command("go", &["fix", "./..."], cwd)?;
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
        let (stdout, _stderr, success) = build::run_command("gofmt", &["-l", "."], cwd)?;
        // gofmt -l prints unformatted file names; empty output = formatted
        Ok(success && stdout.trim().is_empty())
    }

    fn fmt_write(&self, cwd: Option<&Path>) -> ToolResult<()> {
        build::run_command("gofmt", &["-s", "-w", "."], cwd)?;
        Ok(())
    }

    fn doc(&self, _pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<String> {
        // go doc shows package documentation
        let (stdout, stderr, success) = build::run_command("go", &["doc", "./..."], cwd)?;
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
        let (stdout, stderr, success) = build::run_command("go", &["run", "."], cwd)?;
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

fn parse_go_vet_output(_stderr: &str, stdout: &str) -> (Vec<Diag>, Vec<Diag>) {
    // go vet output: file:line:col: message
    let errors = Vec::new();
    let mut warnings = Vec::new();

    for line in stdout.lines() {
        let t = line.trim();
        // Format: ./src/file.go:10:5: some message
        if let Some(pos) = t.find(": ") {
            let loc = &t[..pos];
            let msg = t[pos + 2..].to_string();
            let parts: Vec<&str> = loc.rsplitn(3, ':').collect();
            if parts.len() >= 3 {
                let col = parts[0].parse().ok();
                let line_no = parts[1].parse().ok();
                let file = Some(parts[2].to_string());
                warnings.push(Diag {
                    file,
                    line: line_no,
                    col,
                    level: "warning".into(),
                    code: None,
                    message: msg,
                });
            } else {
                warnings.push(Diag {
                    file: Some(loc.to_string()),
                    line: None,
                    col: None,
                    level: "warning".into(),
                    code: None,
                    message: msg,
                });
            }
        }
    }

    (errors, warnings)
}

fn parse_go_build_output(stderr: &str) -> (Vec<Diag>, Vec<Diag>) {
    // go build errors: file:line:col: message
    let mut errors = Vec::new();
    let warnings = Vec::new();

    for line in stderr.lines() {
        let t = line.trim();
        if let Some(pos) = t.find(": ") {
            let loc = &t[..pos];
            let msg = t[pos + 2..].to_string();
            let parts: Vec<&str> = loc.rsplitn(3, ':').collect();
            if parts.len() >= 3 {
                let col = parts[0].parse().ok();
                let line_no = parts[1].parse().ok();
                let file = Some(parts[2].to_string());
                errors.push(Diag {
                    file,
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

fn parse_staticcheck_output(_stderr: &str, stdout: &str) -> (Vec<Diag>, Vec<Diag>) {
    // staticcheck output: file:line:col: message
    // Format: path/file.go:10:5: SA1000: some message
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for line in stdout.lines() {
        let t = line.trim();
        if let Some(pos) = t.find(": ") {
            let loc = &t[..pos];
            let msg = t[pos + 2..].to_string();
            let parts: Vec<&str> = loc.rsplitn(3, ':').collect();
            if parts.len() >= 3 {
                let col = parts[0].parse().ok();
                let line_no = parts[1].parse().ok();
                let file = Some(parts[2].to_string());

                // Determine severity from code prefix
                let code = msg.split(' ').next().map(std::string::ToString::to_string);
                let is_error = code
                    .as_deref()
                    .is_some_and(|c| c.starts_with("SA"));

                if is_error {
                    errors.push(Diag {
                        file,
                        line: line_no,
                        col,
                        level: "error".into(),
                        code,
                        message: msg,
                    });
                } else {
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
        }
    }

    (errors, warnings)
}

fn parse_go_test_results(stdout: &str, stderr: &str) -> TestResults {
    // go test -v output:
    //   === RUN   TestFoo
    //   --- PASS: TestFoo (0.00s)
    //   --- FAIL: TestBar (0.01s)
    //   FAIL
    //   ok   package 0.02s
    let combined = format!("{stdout}\n{stderr}");
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut failures = Vec::new();

    for line in combined.lines() {
        let t = line.trim();
        if t.starts_with("--- PASS: ") {
            passed += 1;
        } else if t.starts_with("--- FAIL: ") {
            failed += 1;
            // Strip trailing "(duration)" for clean test name
            let raw = t.trim_start_matches("--- FAIL: ").trim();
            let name = raw.split(" (").next().unwrap_or(raw).to_string();
            if !name.is_empty() {
                failures.push(crate::build::TestFailure { name });
            }
        }
    }

    // If no verbose output, try summary: "ok pkg" or "FAIL pkg"
    if passed == 0 && failed == 0 {
        for line in combined.lines() {
            let t = line.trim();
            if t.starts_with("ok ") {
                passed = 1; // package passed
            } else if t.starts_with("FAIL ") {
                failed = 1;
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
    fn test_parse_go_vet_output() {
        let stdout = "./src/main.go:10:5: some issue\n./src/utils.go:1:1: another issue\n";
        let (errors, warnings) = parse_go_vet_output("", stdout);
        assert_eq!(errors.len(), 0);
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].file.as_deref(), Some("./src/main.go"));
        assert_eq!(warnings[0].line, Some(10));
        assert_eq!(warnings[0].col, Some(5));
    }

    #[test]
    fn test_parse_go_test_results() {
        let stdout = "=== RUN   TestFoo\n--- PASS: TestFoo (0.00s)\n=== RUN   TestBar\n--- FAIL: TestBar (0.01s)\nFAIL\n";
        let r = parse_go_test_results(stdout, "");
        assert_eq!(r.passed, 1);
        assert_eq!(r.failed, 1);
        assert_eq!(r.failures.len(), 1);
        assert_eq!(r.failures[0].name, "TestBar");
    }

    #[test]
    fn test_parse_go_test_results_summary() {
        let stdout = "ok   my/pkg  0.02s\n";
        let r = parse_go_test_results(stdout, "");
        assert_eq!(r.passed, 1);
        assert_eq!(r.failed, 0);
    }
}
