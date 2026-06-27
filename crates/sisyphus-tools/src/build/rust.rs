use crate::build::{self, BuildOutput, Diag, LanguageBackend, TestFailure, TestResults};
use crate::{ToolError, ToolResult};
use std::path::Path;

/// Rust build system backend using `cargo`.
#[derive(Debug, Clone, Copy)]
pub struct RustBackend;

impl LanguageBackend for RustBackend {
    fn name(&self) -> &str {
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
                let f = parts.next().map(|s| s.to_string());
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
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_toml(dir: &Path, content: &str) {
        fs::write(dir.join("Cargo.toml"), content).unwrap();
    }

    fn write_src(dir: &Path, content: &str) {
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("lib.rs"), content).unwrap();
    }

    #[test]
    fn test_parse_test_results_all_ok() {
        let stdout = "\n     Running unittests src/lib.rs\n\nrunning 3 tests\ntest a ... ok\ntest b ... ok\ntest c ... ok\n\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\n";
        let r = parse_test_results(stdout, "");
        assert_eq!(r.passed, 3);
        assert_eq!(r.failed, 0);
        assert!(r.failures.is_empty());
    }

    #[test]
    fn test_parse_test_results_with_failures() {
        let stdout = "running 2 tests\ntest a ... ok\ntest b ... FAILED\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored\n\n--- Failures ---\n\nfailures:\n    my_test::test_b\n\n";
        let r = parse_test_results(stdout, "");
        assert_eq!(r.passed, 1);
        assert_eq!(r.failed, 1);
        assert_eq!(r.failures.len(), 1);
        assert_eq!(r.failures[0].name, "my_test::test_b");
    }

    #[test]
    fn test_parse_test_results_with_ignored() {
        let stdout =
            "test result: ok. 512 passed; 0 failed; 4 ignored; 0 measured; 0 filtered out\n";
        let r = parse_test_results(stdout, "");
        assert_eq!(r.passed, 512);
        assert_eq!(r.failed, 0);
        assert_eq!(r.ignored, 4);
    }

    #[test]
    fn test_extract_diags_errors() {
        let stderr = "error[E0425]: cannot find value `x` in this scope\n  --> src/lib.rs:10:5\n   |\n10 |     let y = x;\n   |             ^ not found\n\nerror: aborting due to 1 previous error\n";
        let (errors, warnings) = extract_diags(stderr);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code.as_deref(), Some("E0425"));
        assert_eq!(errors[0].message, "cannot find value `x` in this scope");
        assert_eq!(errors[0].file.as_deref(), Some("src/lib.rs"));
        assert_eq!(errors[0].line, Some(10));
        assert_eq!(errors[0].col, Some(5));
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_extract_diags_warnings() {
        let stderr = "warning: unused variable `x`\n  --> src/lib.rs:5:9\n   |\n5 |     let x = 1;\n   |         ^ help: if you intend to use it: `_x`\n";
        let (errors, warnings) = extract_diags(stderr);
        assert!(errors.is_empty());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].level, "warning");
        assert_eq!(warnings[0].file.as_deref(), Some("src/lib.rs"));
        assert_eq!(warnings[0].line, Some(5));
        assert_eq!(warnings[0].col, Some(9));
    }

    #[test]
    fn test_extract_diags_multiple() {
        let stderr = "error[E0308]: mismatched types\n  --> src/main.rs:20:15\n   |\n20 |     let x: i32 = \"hello\";\n   |                  ^^^^^^^ expected i32, found &str\n\nwarning: unused import\n  --> src/main.rs:1:1\n   |\n1 | use std::collections::HashMap;\n   |\n";
        let (errors, warnings) = extract_diags(stderr);
        assert_eq!(errors.len(), 1);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_check_on_valid_project() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        write_toml(
            path,
            r#"[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"
"#,
        );
        write_src(path, "pub fn hello() -> &'static str { \"hello\" }\n");
        let backend = RustBackend;
        let result = backend.check(None, false, Some(path)).unwrap();
        assert!(result.success);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_check_on_invalid_project() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        write_toml(
            path,
            r#"[package]
name = "bad-crate"
version = "0.1.0"
edition = "2021"
"#,
        );
        write_src(path, "pub fn hello() -> &'static str { does_not_exist }\n");
        let backend = RustBackend;
        let result = backend.check(None, false, Some(path)).unwrap();
        assert!(!result.success);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_fmt_check_formatted() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        write_toml(
            path,
            r#"[package]
name = "fmt-crate"
version = "0.1.0"
edition = "2021"
"#,
        );
        write_src(path, "pub fn foo() -> u8 {\n    42\n}\n");
        let backend = RustBackend;
        let ok = backend.fmt_check(Some(path)).unwrap();
        assert!(ok);
    }

    #[test]
    fn test_fmt_check_unformatted() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        write_toml(
            path,
            r#"[package]
name = "bad-fmt-crate"
version = "0.1.0"
edition = "2021"
"#,
        );
        write_src(path, "pub fn foo() -> u8 { 42 }\n");
        let backend = RustBackend;
        // First check it
        backend.fmt_check(Some(path)).unwrap();
        // Then break formatting
        fs::write(path.join("src/lib.rs"), "pub fn foo() -> u8 {\n42\n}\n").unwrap();
        let ok = backend.fmt_check(Some(path)).unwrap();
        assert!(!ok);
    }

    #[test]
    fn test_build_valid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        write_toml(
            path,
            r#"[package]
name = "build-crate"
version = "0.1.0"
edition = "2021"
"#,
        );
        write_src(path, "pub fn works() -> i32 { 1 + 2 }\n");
        let backend = RustBackend;
        let result = backend.build(None, false, Some(path)).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_test_simple() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        write_toml(
            path,
            r#"[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"
"#,
        );
        let src = path.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("lib.rs"),
            r#"
#[test]
fn it_works() { assert_eq!(2 + 2, 4); }

#[test]
fn it_also_works() { assert!(true); }
"#,
        )
        .unwrap();
        let backend = RustBackend;
        let results = backend.test(None, None, false, Some(path)).unwrap();
        assert_eq!(results.passed, 2);
        assert_eq!(results.failed, 0);
    }

    #[test]
    fn test_test_with_failures() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        write_toml(
            path,
            r#"[package]
name = "fail-crate"
version = "0.1.0"
edition = "2021"
"#,
        );
        let src = path.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("lib.rs"),
            r#"
#[test]
fn passing() { assert!(true); }

#[test]
fn failing() { assert!(false, "intentional failure"); }
"#,
        )
        .unwrap();
        let backend = RustBackend;
        let results = backend.test(None, None, false, Some(path)).unwrap();
        assert_eq!(results.passed, 1);
        assert_eq!(results.failed, 1);
        assert_eq!(results.failures.len(), 1);
    }

    #[test]
    fn test_backend_name() {
        let backend = RustBackend;
        assert_eq!(backend.name(), "rust");
    }
}
