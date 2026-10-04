use crate::build::{self, BuildOutput, Diag, LanguageBackend, TestResults};
use sisyphus_tools_core::{ToolError, ToolResult};
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

    /// New gradle.
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
        let (stdout, stderr, success) = build::run_command(&self.tool, &args, cwd)?;
        // 实测（mvn 3.10 + surefire 3.2.5）：**编译失败时一条 `Tests run`
        // 都不打印**（整个输出 `Tests run` 出现 0 次），只有
        // `[ERROR] COMPILATION ERROR :` 与 `BUILD FAILURE`。
        // 此时 `parse_mvn_test_results` 如实给出 0/0，而调用方
        // （`sisyphus-cli/src/build.rs` 的 `if r.failed == 0 { "ok" }`）
        // 会把编译失败显示成「0 个测试全部通过」。
        //
        // 判据用 maven 自己打的 `COMPILATION ERROR` 标记，而不是「没有
        // Tests run 行」——后者为真时，**跑完了但零用例**（空工程、没有
        // 测试类、被 -Dtest 过滤掉）也成立，会误报。
        let combined = build::strip_ansi(&format!("{stdout}\n{stderr}"));
        if !success && combined.contains("COMPILATION ERROR") {
            return Err(ToolError::Generic(format!(
                "`{}` 未能执行任何测试（编译阶段失败）。原始诊断：\n{}",
                self.tool,
                combined.trim()
            )));
        }
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
                        let (line_no, col) =
                            between.split_once(',').map_or((None, None), |(l, c)| {
                                (l.trim().parse().ok(), c.trim().parse().ok())
                            });
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
    // Maven test output（实测 mvn 3.10 + surefire 3.2.5）:
    //   [INFO] Tests run: 10, Failures: 1, Errors: 0, Skipped: 0
    //   [INFO] Tests run: 10, Failures: 1, Errors: 0, Skipped: 0, Time elapsed: 0.1 s -- in probe.MyTest
    //   [ERROR] Failed tests:
    //   [ERROR]   probe.MyTest#testMethod
    //
    // 两个要点，都是被真实输出打脸后改的：
    //
    // 1. **每行都带 `[INFO] ` / `[ERROR] ` 前缀**，所以判据必须先剥前缀，
    //    不能用 `starts_with("Tests run:")`——那样一条都匹配不上，
    //    通过的项目会被报成 passed=0 / failed=0。
    // 2. `Tests run:` 在同一份输出里出现**多次**（每个测试类一次 + 末尾汇总一次），
    //    按类逐条累加会把计数翻倍，所以取**最大**的那次作为权威值。
    let combined = format!("{stdout}\n{stderr}");
    let mut total = 0usize;
    let mut failures_count = 0usize;
    let mut errors_count = 0usize;
    let mut skipped = 0usize;
    let mut failures = Vec::new();

    for line in combined.lines() {
        let t = strip_maven_prefix(line);
        if t.starts_with("Failed tests:") || t.starts_with("Tests in error:") {
            let rest = t.split_once(':').map(|(_, r)| r).unwrap_or("").trim();
            for name in rest.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                failures.push(crate::build::TestFailure {
                    name: name.to_string(),
                });
            }
        }
    }

    // 汇总行：取最大值，类级行自然被末尾汇总行盖掉。
    for line in combined.lines() {
        let t = strip_maven_prefix(line);
        let Some(rest) = t.strip_prefix("Tests run:") else {
            continue;
        };
        // `Tests run: 10` 的 10 紧跟前缀。注意要先按逗号切开再取，
        // 直接 `split_whitespace().next()` 拿到的是 `"1,"`（带逗号），
        // 解析必定失败——这正是「通过项目报成 0」的根因。
        for part in rest.split(',') {
            let p = part.trim();
            if let Ok(n) = p.parse::<usize>() {
                total = total.max(n);
            }
        }
        for part in rest.split(',') {
            let p = part.trim();
            // `Failures: 0` 里第一个空白分隔的 token 是 `Failures:` 而不是数字，
            // 所以数字要从**冒号后**取，不能取首个 token。
            let Some((label, num)) = p.rsplit_once(':') else {
                continue;
            };
            let label = label.trim();
            let Ok(n) = num.trim().parse::<usize>() else {
                continue;
            };
            if label == "Failures" {
                failures_count = failures_count.max(n);
            } else if label == "Errors" {
                errors_count = errors_count.max(n);
            } else if label == "Skipped" {
                skipped = skipped.max(n);
            }
        }
    }

    // `Errors` 与 `Failures` 同属没通过；`Tests run` 含两者，
    // 故通过数要减掉两者。
    let failed = failures_count + errors_count;

    TestResults {
        passed: total.saturating_sub(failed),
        failed,
        ignored: skipped,
        measured: 0,
        filtered_out: 0,
        failures,
        raw_stdout: stdout.to_string(),
        raw_stderr: stderr.to_string(),
    }
}

/// 剥掉 maven 输出行首的 `[INFO] ` / `[ERROR] ` / `[WARNING] ` / `[DEBUG] ` 前缀。
///
/// maven 每行输出都带这类前缀，不剥的话任何 `starts_with` 判据都落空。
fn strip_maven_prefix(line: &str) -> &str {
    let t = line.trim();
    for p in ["[INFO] ", "[ERROR] ", "[WARNING] ", "[DEBUG] "] {
        if let Some(rest) = t.strip_prefix(p) {
            return rest.trim();
        }
    }
    t
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

    /// 回归（真实 mvn 3.10 + surefire 3.2.5 输出抓取，非构造）：
    /// 通过时 stdout 是
    /// `[INFO] Tests run: 1, Failures: 0, Errors: 0, Skipped: 0`。
    /// 行首的 `[INFO] ` 前缀使旧的 `starts_with("Tests run:")` 永不命中，
    /// 于是通过的项目被报成 passed=0 / failed=0。
    #[test]
    fn test_parse_mvn_real_passing_output_is_not_zero() {
        let stdout = "\
[INFO] --- surefire:3.2.5:test (default-test) @ probe ---
[INFO] Running probe.OkTest
[INFO] Tests run: 1, Failures: 0, Errors: 0, Skipped: 0, Time elapsed: 0.063 s -- in probe.OkTest
[INFO]
[INFO] Results:
[INFO]
[INFO] Tests run: 1, Failures: 0, Errors: 0, Skipped: 0
[INFO]
[INFO] BUILD SUCCESS
";
        let r = parse_mvn_test_results(stdout, "");
        assert_eq!(r.passed, 1, "真实通过输出必须计 1，旧解析器报 0");
        assert_eq!(r.failed, 0);
    }

    /// 回归：编译失败时 mvn **一条 `Tests run` 都不打印**（实测 0 行），
    /// 只有 `[ERROR] COMPILATION ERROR :` 与 `BUILD FAILURE`。
    /// 解析器必须据此区分「编译失败」与「跑了但零用例」。
    #[test]
    fn test_parse_mvn_compile_failure_has_no_tests_run_line() {
        let stdout = "\
[ERROR] COMPILATION ERROR :
[INFO] -------------------------------------------------------------
[ERROR] /tmp/probe/src/main/java/probe/Broken.java:[5,9] cannot find symbol
  symbol:   method undefinedFunc()
[INFO] 1 error
[INFO] -------------------------------------------------------------
[INFO] BUILD FAILURE
[INFO] Total time:  2.434 s
[ERROR] Failed to execute goal org.apache.maven.plugins:maven-compiler-plugin:3.16.0:compile (default-compile) on project probe: Compilation failure
";
        let r = parse_mvn_test_results(stdout, "");
        assert_eq!(
            (r.passed, r.failed),
            (0, 0),
            "编译失败时解析器只能报 0/0——真谓词在 test() 层的守卫，不在这里"
        );
        assert!(
            stdout.contains("COMPILATION ERROR"),
            "输入确实是编译失败输出"
        );
    }

    /// 失败计数：`Errors` 也属于没通过，不能只数 `Failures`。
    #[test]
    fn test_parse_mvn_errors_count_as_failed() {
        let stdout = "\
[INFO] Results:
[INFO] Tests run: 3, Failures: 1, Errors: 1, Skipped: 0
[ERROR] Failed tests:
[ERROR]   probe.OkTest#passes
";
        let r = parse_mvn_test_results(stdout, "");
        assert_eq!(r.failed, 2, "Failures 1 + Errors 1 都要算失败");
        assert_eq!(r.passed, 1);
    }
}
