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
        let has_pyproject = cwd.is_some_and(|d| d.join("pyproject.toml").exists());
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
        let (stdout, stderr, success) = build::run_command(&self.python, &args, cwd)?;
        // 实测（pytest 9.1）：收集阶段失败时**退出码是 2**（EXIT_INTERRUPTED），
        // stdout 含 `error during collection` 与 `ERROR collecting <file>`，
        // 一条 `PASSED` 行都没有。`parse_pytest_results` 对此如实给出 0/0，
        // 而调用方（`sisyphus-cli/src/build.rs` 的 `if r.failed == 0 { "ok" }`）
        // 会把收集失败显示成「0 个测试全部通过」。故此处如实报错。
        //
        // 判据只能看**收集错误标记**，不能只看退出码：pytest 的退出码里
        // 0=全过 / 1=有测试失败 / 2=中断（收集错误等）/ 5=跑完但零测试。
        // 2 既可能是收集失败、也可能是运行中被 Ctrl-C 或 `xfail(strict)`
        // 中断；退出码 1 与 5 在这两种情形下同样是合法的，因此单看退出码
        // 无法判定「有没有测试被执行过」。
        //
        // 也不能拿「输出里没有 passed 字样」当补充条件——实测两个假阳性：
        //
        // 1. **fixture 抛错**：测试确实被跑了，只是 setup 阶段报错。输出为
        //    `ERROR test_use.py::test_uses_fixture` 与 `1 error in 1.2s`，
        //    同样含 `ERROR ` 但**不含** `passed`，会被误报成「收集阶段出错」。
        // 2. **路径含 passed**：目录叫 `pytest_named_passed/` 时，输出里的
        //    `rootdir: .../pytest_named_passed` 就让 `passed` 出现了，于是
        //    真正的收集错误反而被漏判成 0/0。
        //
        // 故只用 `error during collection` / `ERROR collecting` 这两个
        // pytest 自己在**收集阶段**才打印的标记（用例级错误用的是
        // `ERROR ` 行），两者都不出现时一律照常解析。
        let combined = format!("{stdout}\n{stderr}");
        if !success
            && (combined.contains("error during collection")
                || combined.contains("ERROR collecting"))
        {
            return Err(ToolError::Generic(format!(
                "`pytest` 未能执行任何测试（收集阶段出错）。pytest 原始诊断：\n{}",
                build::strip_ansi(&combined).trim()
            )));
        }
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
    let mut skipped = 0usize;
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

    // 权威汇总行：`== 1 passed, 2 failed, 3 skipped in 0.5s ==`（也可能是
    // `=== no tests ran in 0.1s ===`、`=== 1 error in 0.3s ===`）。
    //
    // 必须按逗号切分后再看后缀：单独一行只有 `1 passed in 0.36s` 时，
    // 整行并不以 `" passed"` 结尾（结尾是 ` in 0.36s`），所以旧写法
    // `part.strip_suffix(" passed")` 永远不命中，全通过的项目会被报成
    // passed=0，进而让 `if r.failed == 0 { "ok" }` 把 0 个测试显示成通过。
    for line in combined.lines() {
        let t = line.trim().trim_matches('=').trim();
        let Some((counts, tail)) = t.split_once(" in ") else {
            continue;
        };
        if !(tail.ends_with('s') || tail.contains(':')) {
            // 不是 pytest 的耗时尾巴，排除掉别的含 passed 的行。
            continue;
        }
        for part in counts.split(',') {
            let p = part.trim();
            let Some(num) = p.split_whitespace().next() else {
                continue;
            };
            let Ok(n) = num.parse::<usize>() else {
                continue;
            };
            let label = p[num.len()..].trim();
            match label {
                "passed" => passed = passed.max(n),
                "failed" => failed = failed.max(n),
                // fixture 报错在 pytest 里计作 error，同样是没通过的用例。
                "error" | "errors" => failed = failed.max(n),
                "skipped" | "xfailed" | "xpassed" => skipped = skipped.max(n),
                _ => {},
            }
        }
    }

    TestResults {
        passed,
        failed,
        ignored: skipped,
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

    /// 回归：全通过项目输出 `1 passed in 0.36s`，旧后缀匹配不命中，
    /// 曾经把 passed 报成 0（进而让 CLI 把零测试显示成通过）。
    #[test]
    fn test_parse_pytest_single_passing_is_not_zero() {
        let stdout = "\ntest_ok.py .                    [100%]\n\n\
                       ==== 1 passed in 0.36s ====\n";
        let r = parse_pytest_results(stdout, "");
        assert_eq!(r.passed, 1, "单项全过必须计 1，旧解析器报 0");
        assert_eq!(r.failed, 0);
    }

    /// 回归：同一行同时含 failed 与 passed，两者都要取到确切数字。
    #[test]
    fn test_parse_pytest_mixed_summary_line() {
        let stdout = "=== 1 failed, 1 passed in 0.21s ===\n";
        let r = parse_pytest_results(stdout, "");
        assert_eq!((r.passed, r.failed), (1, 1));
    }

    /// fixture 报错计作 error，归入 failed。
    #[test]
    fn test_parse_pytest_fixture_error_counts_as_failed() {
        let stdout = "=== ERROR: setup boom ===\n=== 1 error in 0.34s ===\n";
        let r = parse_pytest_results(stdout, "");
        assert_eq!(r.failed, 1, "fixture error 应算失败");
        assert_eq!(r.passed, 0);
    }

    /// skipped / xfailed 归入 ignored，不得混入 passed。
    #[test]
    fn test_parse_pytest_skipped_goes_to_ignored() {
        let stdout = "=== 1 passed, 3 skipped in 0.15s ===\n";
        let r = parse_pytest_results(stdout, "");
        assert_eq!(r.passed, 1);
        assert_eq!(r.ignored, 3);
    }

    /// 路径里含 passed 不应污染汇总行的取数。
    #[test]
    fn test_parse_pytest_path_containing_passed_does_not_inflate_counts() {
        let stdout = "rootdir: /tmp/pytest_named_passed\n\
                      === 2 failed in 0.40s ===\n";
        let r = parse_pytest_results(stdout, "");
        assert_eq!(r.failed, 2);
        assert_eq!(r.passed, 0, "rootdir 里的 passed 不是用例计数");
    }

    /// 非汇总行（含 `passed` 字样但无耗时尾巴）不得被当成计数。
    #[test]
    fn test_parse_pytest_ignores_non_summary_lines() {
        let stdout = "some line mentioning passed but no timing\n=== 1 passed in 0.01s ===\n";
        let r = parse_pytest_results(stdout, "");
        assert_eq!(r.passed, 1);
    }

    #[test]
    fn test_parse_colon_location() {
        let (file, line, col) = parse_colon_location("src/app.py:10:5");
        assert_eq!(file.as_deref(), Some("src/app.py"));
        assert_eq!(line, Some(10));
        assert_eq!(col, Some(5));
    }
}
