use crate::build::strip_ansi;
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
        let (stdout, stderr, success) = build::cargo_raw(&args, cwd)?;
        // cargo 失败而输出里没有任何 `test result:` 行，说明**根本没有测试
        // 被执行**——典型情形是编译失败（实测：stdout 0 字节，stderr 只有
        // rustc 诊断与 `could not compile`）。此时 `parse_test_results` 会如实
        // 给出 passed=0 / failed=0，若直接返回，调用方（如 CLI 的
        // `if r.failed == 0 { "ok" }`）会把编译失败当成测试全部通过。
        //
        // 故此处如实报错，不再把「编译失败」伪装成「0 个测试全部通过」。
        if !success && !stdout.contains("test result: ") {
            return Err(ToolError::Generic(format!(
                "`cargo test` 未能执行任何测试（退出码非零）。cargo 原始诊断：\n{}",
                stderr.trim()
            )));
        }
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
//
// `extract_diags` 在解析前统一走 `strip_ansi`（见 `mod.rs`）去掉 ANSI 转义码：
// CI 环境（dtolnay/rust-toolchain）默认设置 `CARGO_TERM_COLOR=always`，
// cargo 输出带色码，直接 `strip_prefix("error[")` 会失败，导致诊断解析为空。

/// Extract diags.
pub fn extract_diags(stderr: &str) -> (Vec<Diag>, Vec<Diag>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut lines = stderr.lines().map(strip_ansi).peekable();

    while let Some(line) = lines.next() {
        let (level, code, msg): (String, Option<String>, String) =
            if let Some(rest) = line.strip_prefix("error[") {
                let code_end = rest.find(']').unwrap_or(0);
                let code = rest[..code_end].to_string();
                let msg = rest[code_end + 1..].trim().trim_start_matches(':').trim().to_string();
                (String::from("error"), Some(code), msg)
            } else if let Some(rest) = line.strip_prefix("error:") {
                if rest.trim().starts_with("aborting due to")
                    || rest.trim().starts_with("could not compile")
                    || rest.trim().contains("For more information about this error")
                {
                    continue;
                }
                (String::from("error"), None, rest.trim().to_string())
            } else if let Some(rest) = line.strip_prefix("warning[") {
                let code_end = rest.find(']').unwrap_or(0);
                let code = rest[..code_end].to_string();
                let msg = rest[code_end + 1..].trim().trim_start_matches(':').trim().to_string();
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

/// 判定 libtest 输出中一个测试块的起始行：`running N test` 或
/// `running N tests`（N 为数字）。
///
/// libtest 在 stdout 中为每个测试目标输出这种行，于是 stdout 天然被切成
/// 「块」结构：起始行 → 各条目行 → `test result:` 汇总行。
fn is_libtest_block_start(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("running ") else {
        return false;
    };
    if let Some(count) = rest.strip_suffix(" tests") {
        return count.parse::<usize>().is_ok();
    }
    if let Some(count) = rest.strip_suffix(" test") {
        return count.parse::<usize>().is_ok();
    }
    false
}

/// 判定 libtest 文档测试的条目行。
///
/// libtest 对文档测试使用 `test <路径> - <条目> (line N)` 命名，例如
/// `test src/lib.rs - documented (line 8) ... ok`。普通单元测试不会命中。
fn is_doc_test_entry(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("test ") else {
        return false;
    };
    // 必须出现 ` - ` 分隔符与 ` (line N)` 位置后缀。
    rest.contains(" - ") && rest.contains(" (line ")
}

/// Parse test results from cargo test output.
///
/// 只统计单元测试目标的结果，不含 `Doc-tests` 段，详见函数体内注释。
pub fn parse_test_results(stdout: &str, stderr: &str) -> TestResults {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut ignored = 0usize;
    let mut measured = 0usize;
    let mut filtered_out = 0usize;
    let mut failures = Vec::new();

    let combined = format!("{stdout}\n{stderr}");
    // `cargo test` 会为每个测试目标各输出一行 `test result:`，其中包含
    // `Doc-tests <crate>` 段。原先把所有这些行一律累加，于是文档测试的
    // 通过数被计入 `passed`。
    //
    // 该缺陷已实测确认：给测试用临时 crate 加一段可运行的文档示例后，
    // cargo 输出 `ok. 2 passed`（unittest）与 `ok. 1 passed`（doc-test），
    // 累加得 passed=3，调用方 `assert_eq!(results.passed, 2)` 随之失败——
    // 单元测试其实全部通过。
    //
    // 不能靠 `Running` / `Doc-tests` 分段标记来区分：这些标记由 cargo 写在
    // **stderr**，而 `test result:` 写在 **stdout**。本函数把两流拼成
    // `format!("{stdout}\n{stderr}")`，所有标记必然排在所有 `test result:`
    // 行之后，基于标记的区间判断在原理上不可能生效（已实测确认）。
    //
    // 可用的判别特征只能在 stdout 内部：libtest 对文档测试条目使用
    // `test <路径> - <条目> (line N)` 命名约定。故按 stdout 中每个
    // `running N test(s)` 起始的块归组，块内条目命中该约定则整块跳过。
    //
    // 零测试的文档测试块不含这样的条目，但它的 `test result:` 恒为
    // `0 passed`，跳过与否都不影响 `passed`，因此这一盲区是安全的。
    let mut block_is_doc_tests = false;
    for line in combined.lines() {
        let trimmed = line.trim();
        if is_libtest_block_start(trimmed) {
            // 新块开始，块类型待定，由紧随其后的条目决定。
            block_is_doc_tests = false;
            continue;
        }
        if is_doc_test_entry(trimmed) {
            block_is_doc_tests = true;
        }
        if trimmed.starts_with("test result: ") && !block_is_doc_tests {
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

    // `failures` 必须与上面计数保持同一口径：文档测试的条目同样不得进入
    // 列表。否则 CLI 会打印一个既没被计入 `failed`、调用方又无法解释的条目。
    //
    // 真实输出里有个容易踩空的事实：libtest 打完失败条目后，紧接着的
    // **空行**就会把 `in_failures` 置回 false。因此下面 `---- ... ----` 与
    // `FAILED` 两个过滤条件实际很少被触发——曾用「去掉它们」做变异，测试
    // 依然全绿，说明那次变异并未真正改变行为，不算有效变异。真正有效的那
    // 一处是 `!in_doc_test_block`：去掉后精确报出混入的文档测试条目。
    let mut in_failures = false;
    let mut in_doc_test_block = false;
    for line in combined.lines() {
        let trimmed = line.trim();
        if trimmed == "failures:" {
            in_failures = true;
            continue;
        }
        if is_libtest_block_start(trimmed) {
            in_doc_test_block = false;
        } else if is_doc_test_entry(trimmed) {
            in_doc_test_block = true;
        }
        if in_failures {
            if trimmed.is_empty() || trimmed.starts_with("test result") {
                in_failures = false;
            } else if !trimmed.starts_with("----")
                && !trimmed.starts_with("FAILED")
                && !in_doc_test_block
            {
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
