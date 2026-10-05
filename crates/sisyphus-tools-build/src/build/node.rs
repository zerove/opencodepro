use crate::build::{self, BuildOutput, Diag, LanguageBackend, TestResults};
use sisyphus_tools_core::{ToolError, ToolResult};
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
    fn name(&self) -> &'static str {
        "node"
    }

    fn check(&self, _pkg: Option<&str>, _all: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
        // TypeScript type-check: tsc --noEmit
        // If no tsconfig, try vue-tsc for Vue projects, or skip
        let has_tsconfig = cwd.is_some_and(|d| d.join("tsconfig.json").exists());
        let has_vue = cwd.is_some_and(|d| d.join("vue.config.js").exists() || has_vue_dep(cwd));

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
        let (stdout, stderr, success) = build::run_command(&self.pm, &args, cwd)?;
        // 实测（vitest 5.0.3）：套件在导入分析阶段就崩掉时退出码 1，且
        //
        // - **`Failed Suites 1` 与 `FAIL  bad.test.js [ bad.test.js ]` 全部
        //   打在 _stderr_**（stdout 里只有 `Test Files  1 failed (1)` 与
        //   `Tests  no tests` 两行）；
        // - 这些标记还被 ANSI 转义码拆开，实际是
        //   `\e[1m\e[41m Failed Suites 1 \e[49m\e[22m`。
        //
        // `parse_vitest_results` 逐行匹配 `passed`/`failed` 字样。此刻 stdout
        // 只有 `Tests  no tests`（**不含** passed/failed 子串），于是如实给出
        // 0/0，而 CLI 的 `if r.failed == 0 { "ok" }` 会把套件加载失败显示成
        // 「0 个测试全部通过」。
        //
        // 因此判据必须 **(1) 跨 stdout+stderr 查找**（只看 stdout 永远不会触发，
        // 本守卫修正前就是这种死代码）、**(2) 先去 ANSI 转义码**、
        // **(3) 不能拿 `Tests  ` 前缀当「有测试跑过」的标记**——实测它在
        // 套件加载失败时**同样存在**（`      Tests  no tests`），用它当反证
        // 会让守卫永远不触发。
        //
        // 实测三组输出（去 ANSI 后）：
        //
        // | 场景            | `Failed Suites` | `no tests`      | `Tests  N passed` |
        // |-----------------|-----------------|-----------------|-------------------|
        // | 套件加载失败     | ✔               | ✔               | ✘                 |
        // | 用例真失败       | ✘               | ✘               | 可能（部分通过时） |
        // | 全部通过         | ✘               | ✘               | ✔                 |
        //
        // 故用 `no tests` 作为「一个用例都没跑」的阳性标记，`Failed Suites`
        // 作为「因加载失败而中止」的阳性标记，两者同时出现才判定为未执行。
        let combined = build::strip_ansi(&format!("{stdout}\n{stderr}"));
        if vitest_suite_never_ran(success, &combined) {
            return Err(ToolError::Generic(format!(
                "`vitest` 未能执行任何测试（套件加载失败）。vitest 原始诊断：\n{}",
                combined.trim()
            )));
        }
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
        String::from_utf8(output.stdout).ok().map(|s| s.trim().to_string())
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

/// 「vitest 一个用例都没跑起来」谓词。`combined` 必须已去掉 ANSI 转义码，
/// 且需包含 stdout **与** stderr（`Failed Suites` 只打在 stderr）。
///
/// 拆成独立函数是为了能用**实测捕获的真实输出**做单测：vitest 需要先
/// `npm install`，不可能进默认 `cargo test`，但它的输出是一份固定文本。
/// 下面的单测直接用 vitest 5.0.3 的真实字节（含 ANSI 转义码）。
fn vitest_suite_never_ran(success: bool, combined: &str) -> bool {
    !success && combined.contains("Failed Suites") && combined.contains("no tests")
}

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
            line.find(": warning ").map(|pos| (pos, ": warning ", false))
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
            let file = result["filePath"].as_str().map(std::string::ToString::to_string);
            if let Some(messages) = result["messages"].as_array() {
                for msg in messages {
                    let line = msg["line"].as_u64().map(|n| n as usize);
                    let col = msg["column"].as_u64().map(|n| n as usize);
                    let severity = msg["severity"].as_u64().unwrap_or(1);
                    let message = msg["message"].as_str().unwrap_or("").to_string();
                    let rule_id = msg["ruleId"].as_str().map(std::string::ToString::to_string);
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

/// 解析 vitest / jest 的测试结果。
///
/// **优先采信 `Tests  N passed | M failed (K)` 汇总行**——只有它带确切数字。
/// 逐行匹配的旧实现在「部分通过」时会算错方向：实测 `Tests  1 failed |
/// 1 passed (2)` 这一行**同时含 `passed` 与 `failed`**，而旧代码先判
/// `contains("passed")` 就 `passed += 1` 并 `continue`，于是
/// `failed` 计数恒为 0、`passed` 虚增。端到端实测：两个用例（一过一败）
/// 报出 `passed=2 failed=0`——**失败的套件被显示成全绿**，比计数不准严重得多。
///
/// 汇总行缺失时（如输出被截断、或非标准 reporter）再退回逐行标记计数，
/// 此时语义是「至少命中一行」，如实反映能拿到的信息。
fn parse_vitest_results(stdout: &str, stderr: &str) -> TestResults {
    // vitest 默认对输出上色，标记会被 ANSI 转义码拆开
    // （`Tests` 变成 `\e[2m Tests \e[22m`），因此解析前统一去色。
    // 不去色时下面 `starts_with("Tests")` / `starts_with("FAIL ")` 等判据
    // 全部落空，只能退回逐行匹配，失败用例名还会带上转义码。
    let combined = build::strip_ansi(&format!("{stdout}\n{stderr}"));
    let mut failures = Vec::new();

    // 汇总行里的一段，如 `1 failed (2)` / `2 passed | 3 skipped (5)`。
    // 先剥掉末尾的 `(总数)`，再拆数字与标签。
    let summary_segment = |seg: &str| -> Option<(usize, &'static str)> {
        // 去掉 ` (2)` 这类总数括号，只留 `1 failed`。
        let seg = match (seg.find('('), seg.rfind(')')) {
            (Some(a), Some(b)) if b > a => format!("{} ", &seg[..a]),
            _ => seg.to_string(),
        };
        let seg = seg.trim();
        let num: String = seg.chars().take_while(char::is_ascii_digit).collect();
        let label = seg[num.len()..].trim().to_ascii_lowercase();
        if num.is_empty() || label.is_empty() {
            return None;
        }
        let n: usize = num.parse().ok()?;
        Some(match label.as_str() {
            "passed" => (n, "passed"),
            "failed" => (n, "failed"),
            "skipped" | "todo" => (n, "skipped"),
            _ => return None,
        })
    };

    let mut summary_found = false;
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut ignored = 0usize;

    for line in combined.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("Tests ").or_else(|| t.strip_prefix("Tests")) else {
            continue;
        };
        // 汇总行以 `(总数)` 结尾，如 `Tests  1 failed | 1 passed (2)`。
        // `no tests` / `no tests` 变体不含括号数字，交给调用方的守卫处理。
        if !rest.contains('(') {
            continue;
        }
        let mut seg_p = None;
        let mut seg_f = None;
        let mut seg_i = None;
        for part in rest.split('|') {
            match summary_segment(part) {
                Some((n, "passed")) => seg_p = Some(n),
                Some((n, "failed")) => seg_f = Some(n),
                Some((n, "skipped")) | Some((n, "todo")) => seg_i = Some(n),
                _ => {},
            }
        }
        // 一段都没解出就不是汇总行（如 `Duration  249ms (transform …)`）。
        if seg_p.is_none() && seg_f.is_none() && seg_i.is_none() {
            continue;
        }
        // 最后一条汇总行生效（TUI 模式会重复打印）。
        passed = seg_p.unwrap_or(0);
        failed = seg_f.unwrap_or(0);
        ignored = seg_i.unwrap_or(0);
        summary_found = true;
    }

    if !summary_found {
        // 退回逐行标记：只能回答「至少有一行命中」。
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
    } else {
        // 汇总行给出确切失败数，但失败用例名在 `Failed Tests N` 段里。
        collect_failed_test_names(&combined, &mut failures);
    }

    TestResults {
        passed,
        failed,
        ignored,
        measured: 0,
        filtered_out: 0,
        failures,
        raw_stdout: stdout.to_string(),
        raw_stderr: stderr.to_string(),
    }
}

/// 抽出 `FAIL  <file> > <case>` 形式的失败用例名。
///
/// 实测 vitest 5.0.3 的 `Failed Tests N` 段落形如（去 ANSI 后，段落标题两侧
/// 还有装饰字符，所以不能用 `starts_with("Failed Tests")`）：
///
/// ```text
/// ⎯⎯⎯⎯⎯⎯⎯ Failed Tests 2 ⎯⎯⎯⎯⎯⎯⎯
///  FAIL  f1.test.js > fails one
/// ```
///
/// 判据用 `FAIL ` + 行内含 `>`，而不是标题匹配：套件加载失败的输出里也有
/// `FAIL  bad.test.js [ bad.test.js ]`，但它**没有** `>`（那是套件名不是用例名），
/// 因此不会混入失败列表。
fn collect_failed_test_names(combined: &str, failures: &mut Vec<crate::build::TestFailure>) {
    for line in combined.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("FAIL ") else {
            continue;
        };
        if let Some((_file, name)) = rest.split_once('>') {
            let name = name.trim();
            if !name.is_empty() {
                failures.push(crate::build::TestFailure {
                    name: name.to_string(),
                });
            }
        }
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
    fn test_backend_name() {
        // `name` is a pure literal on the unit struct — no toolchain needed.
        // `NodeBackend::new()` is what probes for a package manager, so it is
        // that constructor (not this assertion) that needs an installed Node.
        let backend = NodeBackend { pm: "npm".into() };
        assert_eq!(LanguageBackend::name(&backend), "node");
    }
    // ── vitest 守卫：用实测捕获的真实字节锁定判据 ──────────
    //
    // 以下常量逐字节来自 vitest 5.0.3 的真实输出（含 ANSI 转义码）。
    // 捕获方式：`npm test`，测试文件分别写
    //   - `function broken( {`（语法错误 → 套件加载失败）
    //   - `expect(1).toBe(2)`（断言失败）
    //   - `expect(1+1).toBe(2)`（通过）
    // 每一行都是原样搬运，**不做任何手工删改**——本文件第一版就是手工转录
    // 常量，结果 ANSI 序列与字段顺序都写错，断言全部失败。判据必须建立在
    // 真字节上，否则测试本身就在验证一个虚构的输出。

    /// 真实 stderr（语法错误）：`Failed Suites` 段落**只出现在 stderr**。
    const VITEST_BAD_STDERR: &str = concat!(
        "\u{1b}[31m⎯⎯⎯⎯⎯⎯\u{1b}[39m\u{1b}[1m\u{1b}[41m Failed Suites 1 \u{1b}[49m\u{1b}[22m\u{1b}[31m⎯⎯⎯⎯⎯⎯\u{1b}[39m\n",
        "\u{1b}[41m\u{1b}[1m FAIL \u{1b}[22m\u{1b}[49m bad.test.js\u{1b}[2m [ bad.test.js ]\u{1b}[22m\n",
        "\u{1b}[31m\u{1b}[1mError\u{1b}[22m: Failed to parse source for import analysis because the content contains invalid JS syntax. If you are using JSX, make sure to name the file with the .jsx or .tsx extension.\u{1b}[39m\n",
    );

    /// 真实 stdout（语法错误）：只有汇总两行，`Tests` 行是 `no tests`。
    ///
    /// 注意 `      Tests  no tests` **确实以 `Tests  `（两个空格）开头**——
    /// 守卫若拿这个前缀当「有测试跑过」的标记，会永远不触发。
    const VITEST_BAD_STDOUT: &str = concat!(
        "\u{1b}[2m Test Files \u{1b}[22m \u{1b}[1m\u{1b}[31m1 failed\u{1b}[39m\u{1b}[22m\u{1b}[90m (1)\u{1b}[39m\n",
        "\u{1b}[2m      Tests \u{1b}[22m \u{1b}[2mno tests\u{1b}[22m\n",
    );

    /// 真实 stdout（全部通过）。
    const VITEST_GOOD_STDOUT: &str = concat!(
        "\u{1b}[2m Test Files \u{1b}[22m \u{1b}[1m\u{1b}[32m1 passed\u{1b}[39m\u{1b}[22m\u{1b}[90m (1)\u{1b}[39m\n",
        "\u{1b}[2m      Tests \u{1b}[22m \u{1b}[1m\u{1b}[32m1 passed\u{1b}[39m\u{1b}[22m\u{1b}[90m (1)\u{1b}[39m\n",
    );

    /// 真实 stdout（一个用例断言失败、另一个通过）。
    const VITEST_ASSERT_FAIL_STDOUT: &str = concat!(
        "\u{1b}[2m Test Files \u{1b}[22m \u{1b}[1m\u{1b}[31m1 failed\u{1b}[39m\u{1b}[22m\u{1b}[2m | \u{1b}[22m\u{1b}[1m\u{1b}[32m1 passed\u{1b}[39m\u{1b}[22m\u{1b}[90m (2)\u{1b}[39m\n",
        "\u{1b}[2m      Tests \u{1b}[22m \u{1b}[1m\u{1b}[31m1 failed\u{1b}[39m\u{1b}[22m\u{1b}[2m | \u{1b}[22m\u{1b}[1m\u{1b}[32m1 passed\u{1b}[39m\u{1b}[22m\u{1b}[90m (2)\u{1b}[39m\n",
    );

    fn strip(s: &str) -> String {
        build::strip_ansi(s)
    }

    /// 套件加载失败必须被判为「一个用例都没跑」。
    ///
    /// 入参是 **stdout + stderr 合并后再去 ANSI**。两个条件缺一不可：
    /// - `Failed Suites` 只在 stderr —— 只查 stdout 的守卫是死代码；
    /// - `no tests` 是「零用例」的唯一阳性标记 —— `Tests  ` 前缀在失败场景
    ///   下同样存在，不能用来反证。
    #[test]
    fn vitest_suite_load_failure_is_detected() {
        let combined = strip(&format!("{VITEST_BAD_STDOUT}\n{VITEST_BAD_STDERR}"));
        assert!(
            vitest_suite_never_ran(false, &combined),
            "真实的套件加载失败必须被守卫捕获；combined=\n{combined}"
        );
    }

    /// 防回归：把「`Failed Suites` 不在 stdout」这个前提固化成断言。
    ///
    /// 原守卫就是只看 stdout，于是这一条永远不成立、守卫形同虚设。写进
    /// 断言后人若再改回单流判据，本用例会立刻失败。
    #[test]
    fn vitest_failed_suites_marker_is_absent_from_stdout_alone() {
        assert!(
            !VITEST_BAD_STDOUT.contains("Failed Suites"),
            "实测：Failed Suites 不在 stdout——只查 stdout 的守卫是死代码"
        );
        assert!(
            VITEST_BAD_STDERR.contains("Failed Suites"),
            "实测：Failed Suites 在 stderr"
        );
    }

    /// 防回归：套件加载失败时 `Tests  ` 前缀**存在**，因此它不能当反证标记。
    ///
    /// 守卫中间版本用的正是 `!combined.contains("Tests  ")`，实测恒为 false，
    /// 同样永不触发。本用例锁死这一事实。
    #[test]
    fn vitest_tests_prefix_exists_even_when_no_case_ran() {
        let stripped = strip(VITEST_BAD_STDOUT);
        assert!(
            stripped.contains("Tests  "),
            "实测：套件加载失败时仍有 `Tests  ` 前缀，actual:\n{stripped}"
        );
        assert!(
            stripped.contains("no tests"),
            "实测：真正区分「零用例」的是 `no tests`"
        );
    }

    /// 防误伤：全部通过时不得被判成「没跑测试」。
    #[test]
    fn vitest_passing_run_is_not_flagged() {
        let combined = strip(VITEST_GOOD_STDOUT);
        assert!(
            !vitest_suite_never_ran(true, &combined),
            "全部通过不得被守卫拦下"
        );
    }

    /// 防误伤：用例真失败时必须照常解析，**不得**被判成「没跑测试」。
    /// 防误伤：用例真失败时必须照常解析，**不得**被判成「没跑测试」。
    ///
    /// 用例真失败时**不出现** `Failed Suites`（它专指套件加载失败），
    /// 也不出现 `no tests`，因此守卫不触发。
    ///
    /// 汇总行是唯一带确切数字的地方：`Tests  1 failed | 1 passed (2)`
    /// → `failed=1 / passed=1`。旧实现在这一行上先命中 `passed` 分支，
    /// 报出 `failed=0 / passed=2`——**把失败的套件说成全绿**。
    #[test]
    fn vitest_real_test_failure_is_not_flagged_as_never_ran() {
        let combined = strip(VITEST_ASSERT_FAIL_STDOUT);
        assert!(
            !vitest_suite_never_ran(false, &combined),
            "用例真失败必须照常解析，不得被守卫拦成 Err"
        );
        let r = parse_vitest_results(&strip(VITEST_ASSERT_FAIL_STDOUT), "");
        println!("passed={} failed={}", r.passed, r.failed);
        assert_eq!(
            r.failed, 1,
            "汇总行 `Tests  1 failed | 1 passed (2)` 必须解析出 1 个失败"
        );
        assert_eq!(
            r.passed, 1,
            "同一行同时含 passed 与 failed，两者都要取到确切数字"
        );
    }

    /// 全部用例失败时同理：`Tests  2 failed (2)` → `failed=2`。
    #[test]
    fn vitest_all_cases_failing_is_counted_correctly() {
        let r = parse_vitest_results("      Tests  2 failed (2)\n", "");
        assert_eq!(r.failed, 2);
        assert_eq!(r.passed, 0);
    }

    /// `skipped` 必须计入 `ignored`，不得混入 passed——否则 CLI 会把跳过的
    /// 用例显示成「已通过」。
    #[test]
    fn vitest_skipped_is_counted_as_ignored() {
        let r = parse_vitest_results("      Tests  1 failed | 3 skipped (4)\n", "");
        assert_eq!(r.failed, 1);
        assert_eq!(r.passed, 0);
        assert_eq!(r.ignored, 3, "skipped 不得混入 passed");
    }

    /// 失败用例名必须来自 `Failed Tests` 段，而不是把汇总行当成失败项。
    #[test]
    fn vitest_failed_test_names_come_from_failed_tests_section() {
        let out = strip(concat!(
            " FAIL  f1.test.js > fails one\n",
            "AssertionError: expected 1 to be 2\n",
            " Test Files  1 failed (1)\n",
            "      Tests  1 failed (1)\n",
        ));
        let r = parse_vitest_results(&out, "");
        assert_eq!(r.failed, 1);
        assert_eq!(r.failures.len(), 1, "失败项应恰好一个");
        assert_eq!(
            r.failures[0].name, "fails one",
            "失败名应是真实用例名，而不是汇总行"
        );
    }

    /// 汇总行缺失时退回逐行标记，语义是「至少命中一行」。
    #[test]
    fn vitest_falls_back_to_line_markers_without_summary() {
        let r = parse_vitest_results(" ✓ basic test (2ms)\n ✗ failing test (1ms)\n", "");
        assert_eq!(r.passed, 1);
        assert_eq!(r.failed, 1);
        assert_eq!(r.failures[0].name, "failing test");
    }

    /// 套件加载失败时，汇总行是 `Tests  no tests`（**不带括号数字**），解析器
    /// 识别不了它，于是退回逐行标记、把 `Test Files  1 failed` 误计成 1 个
    /// 失败用例。CLI 会显示 `Test result: FAILED. 0 passed; 1 failed`：既错报
    /// 计数又错报性质，把「一个测试都没跑」说成「有 1 个失败」。
    ///
    /// 拦成 Err（`vitest_suite_never_ran`）才是唯一正确处理——这正是守卫
    /// 不可省、且不能用解析器修的原因。
    #[test]
    fn vitest_suite_load_failure_yields_misleading_counts_without_guard() {
        let r = parse_vitest_results(&strip(VITEST_BAD_STDOUT), "");
        println!(
            "passed={} failed={} failures={:?}",
            r.passed,
            r.failed,
            r.failures.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
        assert_eq!(
            r.failed, 1,
            "实测：`Test Files  1 failed` 这行会被误计成 1 个失败用例"
        );
        assert_eq!(r.passed, 0);
        assert!(
            r.failures.iter().any(|f| f.name.contains("Test Files")),
            "失败项实为汇总行而非用例名，actual: {:?}",
            r.failures.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
    }
}
