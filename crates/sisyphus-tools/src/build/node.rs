use crate::build::{self, BuildOutput, Diag, LanguageBackend, TestResults};
use crate::{ToolError, ToolResult};
use std::path::Path;

/// Node.js / TypeScript frontend backend.
///
/// Uses `tsc` for type-checking, `eslint` for linting,
/// `prettier` for formatting, and `vitest` / `jest` for testing.
#[derive(Debug)]
pub struct NodeBackend {
    /// Package manager: "npm", "yarn", or "pnpm"
    pm: String,
}

impl NodeBackend {
    pub fn new() -> ToolResult<Self> {
        // Auto-detect package manager
        for pm in &["pnpm", "yarn", "npm"] {
            if build::run_command(pm, &["--version"], None).is_ok() {
                return Ok(Self { pm: pm.to_string() });
            }
        }
        Err(ToolError::Generic(
            "No package manager found (npm/yarn/pnpm)".into(),
        ))
    }
}

impl LanguageBackend for NodeBackend {
    fn name(&self) -> &str {
        "node"
    }

    fn check(&self, _pkg: Option<&str>, _all: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        // TypeScript type-check: tsc --noEmit
        // If no tsconfig, try vue-tsc for Vue projects, or skip
        let has_tsconfig = cwd
            .map(|d| d.join("tsconfig.json").exists())
            .unwrap_or(false);
        let has_vue = cwd
            .map(|d| d.join("vue.config.js").exists() || has_vue_dep(cwd))
            .unwrap_or(false);

        if has_vue && which("vue-tsc").is_some() {
            let (stdout, stderr, success) = build::run_command("vue-tsc", &["--noEmit"], cwd)?;
            let (errors, warnings) = parse_tsc_diags(&stderr);
            return Ok(BuildOutput {
                errors,
                warnings,
                raw_stdout: stdout,
                raw_stderr: stderr,
                success,
            });
        }

        if has_tsconfig && which("tsc").is_some() {
            let (stdout, stderr, success) = build::run_command("tsc", &["--noEmit"], cwd)?;
            let (errors, warnings) = parse_tsc_diags(&stderr);
            return Ok(BuildOutput {
                errors,
                warnings,
                raw_stdout: stdout,
                raw_stderr: stderr,
                success,
            });
        }

        // No type-checker available — pass (no errors)
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
        let (stdout, stderr, success) = build::run_command(&self.pm, &["run", "build"], cwd)?;
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
        let mut args = vec!["run", "test"];
        if let Some(n) = name {
            args.push("--");
            args.push(n);
        }
        let (stdout, stderr, _success) = build::run_command(&self.pm, &args, cwd)?;
        Ok(parse_vitest_results(&stdout, &stderr))
    }

    fn lint(&self, _pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        if which("eslint").is_none() {
            return Ok(BuildOutput {
                errors: vec![],
                warnings: vec![],
                raw_stdout: String::new(),
                raw_stderr: String::new(),
                success: true,
            });
        }
        // eslint outputs JSON by default when available, else use flat config
        let (stdout, stderr, success) =
            build::run_command("eslint", &[".", "--format", "json"], cwd)?;
        let (errors, warnings) = parse_eslint_json(&stdout);
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
        apply_lint_fixes: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput> {
        if apply_lint_fixes && which("eslint").is_some() {
            build::run_command("eslint", &[".", "--fix"], cwd)?;
        }
        // Also run prettier if available
        if which("prettier").is_some() {
            build::run_command("prettier", &["--write", "."], cwd)?;
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
        if which("prettier").is_some() {
            let (_stdout, _stderr, success) =
                build::run_command("prettier", &["--check", "."], cwd)?;
            return Ok(success);
        }
        Ok(true) // no formatter = assume formatted
    }

    fn fmt_write(&self, cwd: Option<&Path>) -> ToolResult<()> {
        if which("prettier").is_some() {
            build::run_command("prettier", &["--write", "."], cwd)?;
        }
        Ok(())
    }

    fn doc(&self, _pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<String> {
        if which("typedoc").is_some() {
            let (stdout, stderr, success) = build::run_command("typedoc", &[], cwd)?;
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
            Ok("typedoc not installed — no docs generated".into())
        }
    }

    fn run(&self, args: &[&str], cwd: Option<&Path>) -> ToolResult<String> {
        let script = if args.is_empty() { "start" } else { args[0] };
        let extra = if args.len() > 1 { &args[1..] } else { &[] };
        let mut cmd_args = vec!["run", script];
        cmd_args.extend_from_slice(extra);
        let (stdout, stderr, success) = build::run_command(&self.pm, &cmd_args, cwd)?;
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

// ── Helpers ───────────────────────────────────────────

fn which(cmd: &str) -> Option<String> {
    let output = std::process::Command::new("which").arg(cmd).output().ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

fn has_vue_dep(cwd: Option<&Path>) -> bool {
    let pkg = cwd.map(|d| d.join("package.json")).and_then(|p| {
        let content = std::fs::read_to_string(p).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        Some(json)
    });
    if let Some(json) = pkg {
        let deps = json.get("dependencies").and_then(|d| d.as_object());
        let dev_deps = json.get("devDependencies").and_then(|d| d.as_object());
        deps.and_then(|d| d.get("vue")).is_some() || dev_deps.and_then(|d| d.get("vue")).is_some()
    } else {
        false
    }
}

// ── Parsers ───────────────────────────────────────────

fn parse_tsc_diags(stderr: &str) -> (Vec<Diag>, Vec<Diag>) {
    // tsc format: src/file.ts(line,col): error TS2345: message
    // or: src/file.ts:line:col - error TS2345: message
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for line in stderr.lines() {
        // Split on " - error " or ": error " or " - warning " or ": warning "
        let error_split = if let Some(pos) = line.find(" - error ") {
            Some((pos, " - error ", true))
        } else {
            line.find(": error ").map(|pos| (pos, ": error ", true))
        };

        let warn_split = if error_split.is_some() {
            None
        } else if let Some(pos) = line.find(" - warning ") {
            Some((pos, " - warning ", false))
        } else {
            line.find(": warning ")
                .map(|pos| (pos, ": warning ", false))
        };

        if let Some((pos, sep, is_error)) = error_split.or(warn_split) {
            let file_part = &line[..pos];
            let msg_part = &line[pos + sep.len()..];

            let (file, line_no, col) = parse_tsc_location(file_part);

            let msg = msg_part.trim().to_string();

            // Extract code (e.g., "TS2345" from "TS2345: message")
            let code = if let Some(end) = msg.find(':') {
                let candidate = &msg[..end];
                if candidate.starts_with("TS")
                    || candidate.starts_with("error TS")
                    || candidate.starts_with("warning TS")
                {
                    Some(
                        candidate
                            .trim_start_matches("error ")
                            .trim_start_matches("warning ")
                            .to_string(),
                    )
                } else {
                    None
                }
            } else {
                None
            };

            let level = if is_error { "error" } else { "warning" };
            let diag = Diag {
                file,
                line: line_no,
                col,
                level: level.into(),
                code,
                message: msg,
            };
            if is_error {
                errors.push(diag);
            } else {
                warnings.push(diag);
            }
        }
    }

    (errors, warnings)
}

fn parse_tsc_location(s: &str) -> (Option<String>, Option<usize>, Option<usize>) {
    // Format: "src/file.ts:10:5" or "src/file.ts(10,5)"
    if let Some(end) = s.rfind(':') {
        let col = s[end + 1..].parse().ok();
        let rest = &s[..end];
        if let Some(mid) = rest.rfind(':') {
            let line = rest[mid + 1..].parse().ok();
            let file = Some(rest[..mid].to_string());
            return (file, line, col);
        }
    }
    // Try paren format: file(10,5)
    if let Some(end) = s.rfind('(') {
        let inner = &s[end + 1..];
        if let Some(comma) = inner.find(',') {
            let line = inner[..comma].trim().parse().ok();
            let col = inner[comma + 1..].trim().trim_end_matches(')').parse().ok();
            return (Some(s[..end].to_string()), line, col);
        }
    }
    (Some(s.to_string()), None, None)
}

fn parse_eslint_json(json_str: &str) -> (Vec<Diag>, Vec<Diag>) {
    // eslint --format json outputs array of {filePath, messages: [{line,column,severity,message,ruleId}]}
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if let Ok(results) = serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
        for result in results {
            let file = result["filePath"].as_str().map(|s| s.to_string());
            if let Some(messages) = result["messages"].as_array() {
                for msg in messages {
                    let line = msg["line"].as_u64().map(|n| n as usize);
                    let col = msg["column"].as_u64().map(|n| n as usize);
                    let severity = msg["severity"].as_u64().unwrap_or(1);
                    let message = msg["message"].as_str().unwrap_or("").to_string();
                    let rule_id = msg["ruleId"].as_str().map(|s| s.to_string());
                    let level = if severity == 2 { "error" } else { "warning" };

                    let diag = Diag {
                        file: file.clone(),
                        line,
                        col,
                        level: level.into(),
                        code: rule_id,
                        message,
                    };
                    if severity == 2 {
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

fn parse_vitest_results(stdout: &str, stderr: &str) -> TestResults {
    // Vitest/Jest output example:
    //   Tests  1 passed (1)
    // Or:
    //   Tests  1 failed (1)
    // Or multi-line suite output:
    //   ✓ basic test (2ms)
    //   ✗ failing test (1ms)
    //
    // We do a simple count from test summary lines.
    let combined = format!("{stdout}\n{stderr}");
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut failures = Vec::new();

    for line in combined.lines() {
        let t = line.trim();
        if t.starts_with('✓') || t.starts_with("PASS") || t.contains("passed") {
            passed += 1;
        } else if t.starts_with('✗') || t.starts_with("FAIL") || t.contains("failed") {
            failed += 1;
            // Extract test name: "✗ failing test (1ms)" -> "failing test"
            let name = t
                .trim_start_matches('�')
                .trim_start_matches('✗')
                .trim()
                .trim_start_matches("FAIL")
                .trim()
                .split('(')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !name.is_empty() {
                failures.push(crate::build::TestFailure { name });
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
    fn test_parse_tsc_diags() {
        let stderr = "src/app.ts:10:5 - error TS2345: Type 'string' is not assignable to type 'number'.\nsrc/utils.ts:1:1 - warning TS6192: All imports are unused.\n";
        let (errors, warnings) = parse_tsc_diags(stderr);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file.as_deref(), Some("src/app.ts"));
        assert_eq!(errors[0].line, Some(10));
        assert_eq!(errors[0].col, Some(5));
        assert_eq!(errors[0].code.as_deref(), Some("TS2345"));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].file.as_deref(), Some("src/utils.ts"));
    }

    #[test]
    fn test_parse_tsc_diags_paren_format() {
        let stderr = "src/app.ts(10,5): error TS2345: Type mismatch.\n";
        let (errors, _warnings) = parse_tsc_diags(stderr);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file.as_deref(), Some("src/app.ts"));
        assert_eq!(errors[0].line, Some(10));
        assert_eq!(errors[0].col, Some(5));
    }

    #[test]
    #[ignore = "requires actual Node.js toolchain installed"]
    fn test_backend_name() {
        // Can't instantiate without actual tools, but we can test parsing
    }
}
