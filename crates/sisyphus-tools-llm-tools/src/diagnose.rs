use metrics::counter;
use sisyphus_tools_build::build::{self, BuildOutput, Diag, LanguageBackend};
use sisyphus_tools_core::ToolResult;
use std::collections::BTreeMap;
use std::path::Path;

/// A combined diagnosis report from check + lint operations.
#[derive(Debug, Clone)]
pub struct DiagnoseReport {
    /// Output from the `check` operation.
    pub check_output: BuildOutput,
    /// Output from the `lint` operation (None if linter was skipped).
    pub lint_output: Option<BuildOutput>,
    /// Deprecated alias for `lint_output` (to avoid breaking old callers).
    pub clippy_output: Option<BuildOutput>,
    /// Total errors across all sources.
    pub total_errors: usize,
    /// Total warnings across all sources.
    pub total_warnings: usize,
    /// Errors grouped by file path (file → count).
    pub errors_by_file: Vec<(String, usize)>,
    /// Warnings grouped by file path (file → count).
    pub warnings_by_file: Vec<(String, usize)>,
    /// All diagnostics merged, sorted by (file, line).
    pub all_diagnostics: Vec<Diag>,
}

/// Run a combined diagnosis: `check` then `lint` if check passes.
///
/// Uses the given language backend. If `backend` is `None`, auto-detects.
pub fn diagnose_with(
    backend: &dyn LanguageBackend,
    pkg: Option<&str>,
    all: bool,
    no_lint: bool,
    cwd: Option<&Path>,
) -> ToolResult<DiagnoseReport> {
    let check_output = backend.check(pkg, all, cwd)?;

    let mut lint_output = None;
    if !no_lint && check_output.success {
        lint_output = Some(backend.lint(pkg, cwd)?);
    }

    // Merge all diagnostics
    let mut all_diags: Vec<Diag> = check_output.errors.clone();
    all_diags.extend(check_output.warnings.iter().cloned());
    if let Some(ref lint) = lint_output {
        let existing: Vec<(Option<String>, Option<usize>, String)> =
            all_diags.iter().map(|d| (d.file.clone(), d.line, d.message.clone())).collect();
        for d in lint.errors.iter().chain(lint.warnings.iter()) {
            let is_dup = existing.contains(&(d.file.clone(), d.line, d.message.clone()));
            if !is_dup {
                all_diags.push(d.clone());
            }
        }
    }

    // Sort by (file, line)
    all_diags.sort_by(|a, b| {
        a.file
            .as_deref()
            .unwrap_or("")
            .cmp(b.file.as_deref().unwrap_or(""))
            .then(a.line.unwrap_or(0).cmp(&b.line.unwrap_or(0)))
            .then(a.col.unwrap_or(0).cmp(&b.col.unwrap_or(0)))
    });

    // Group by file
    let mut errors_by_file: BTreeMap<String, usize> = BTreeMap::new();
    let mut warnings_by_file: BTreeMap<String, usize> = BTreeMap::new();
    for d in &all_diags {
        let f = d.file.clone().unwrap_or_else(|| "<unknown>".to_string());
        if d.level == "error" {
            *errors_by_file.entry(f).or_insert(0) += 1;
        } else {
            *warnings_by_file.entry(f).or_insert(0) += 1;
        }
    }

    let total_errors = all_diags.iter().filter(|d| d.level == "error").count();
    let total_warnings = all_diags.iter().filter(|d| d.level == "warning").count();

    let clippy_output = lint_output.clone();

    Ok(DiagnoseReport {
        check_output,
        lint_output,
        clippy_output,
        total_errors,
        total_warnings,
        errors_by_file: errors_by_file.into_iter().collect(),
        warnings_by_file: warnings_by_file.into_iter().collect(),
        all_diagnostics: all_diags,
    })
}

/// Auto-detect language backend and diagnose.
pub fn diagnose_auto(
    pkg: Option<&str>,
    all: bool,
    no_lint: bool,
    cwd: Option<&Path>,
) -> ToolResult<DiagnoseReport> {
    let dir = cwd.unwrap_or(Path::new("."));
    let backend = build::detect_backend(dir)?;
    diagnose_with(backend.as_ref(), pkg, all, no_lint, cwd)
}

/// Backward-compatible: runs diagnose with RustBackend (cargo check + clippy).
/// Run diagnose.
pub fn diagnose(
    pkg: Option<&str>,
    all: bool,
    no_clippy: bool,
    cwd: Option<&Path>,
) -> ToolResult<DiagnoseReport> {
    counter!("sisyphus_diagnose_runs_total").increment(1);
    diagnose_with(&build::rust::RustBackend, pkg, all, no_clippy, cwd)
}

/// Run `fix` then re-diagnose, using the given backend.
pub fn fix_and_diagnose_with(
    backend: &dyn LanguageBackend,
    pkg: Option<&str>,
    fix_lint: bool,
    all: bool,
    cwd: Option<&Path>,
) -> ToolResult<DiagnoseReport> {
    backend.fix(pkg, fix_lint, cwd)?;
    diagnose_with(backend, pkg, all, !fix_lint, cwd)
}

/// Auto-detect language backend, fix, and re-diagnose.
pub fn fix_and_diagnose_auto(
    pkg: Option<&str>,
    fix_lint: bool,
    all: bool,
    cwd: Option<&Path>,
) -> ToolResult<DiagnoseReport> {
    let dir = cwd.unwrap_or(Path::new("."));
    let backend = build::detect_backend(dir)?;
    fix_and_diagnose_with(backend.as_ref(), pkg, fix_lint, all, cwd)
}

// ── 验证-修复循环（有界）────────────────────────────────────────

/// 验证-修复循环的参数。
#[derive(Debug, Clone, Default)]
pub struct VerifyFixOptions<'a> {
    /// 目标包。
    pub pkg: Option<&'a str>,
    /// 诊断全部工作区成员。
    pub all: bool,
    /// 跳过 lint（仅 check）。
    pub no_lint: bool,
    /// fix 阶段同时应用 lint 修复。
    pub fix_lint: bool,
    /// 允许的最大警告数；`Some(0)` 等价于"零警告"门禁，`None` 不限制。
    pub max_warnings: Option<usize>,
    /// 最大验证轮数；`0` 视为 `1`。
    pub max_rounds: usize,
}

impl VerifyFixOptions<'_> {
    /// 生效的轮数上限（至少 1）。
    pub fn rounds(&self) -> usize {
        self.max_rounds.max(1)
    }

    /// 判断一次诊断是否通过门禁。
    pub fn is_clean(&self, report: &DiagnoseReport) -> bool {
        if report.total_errors > 0 {
            return false;
        }
        match self.max_warnings {
            Some(limit) => report.total_warnings <= limit,
            None => true,
        }
    }
}

/// 单次修复动作的记录。
#[derive(Debug, Clone)]
pub struct FixAttempt {
    /// `fix` 是否成功退出。
    pub success: bool,
    /// `fix` 自身报告的错误数。
    pub errors: usize,
    /// 耗时（毫秒）。
    pub duration_ms: u128,
    /// 失败信息（成功时为 `None`）。
    pub message: Option<String>,
}

/// 单轮验证的记录。
#[derive(Debug, Clone)]
pub struct VerifyRound {
    /// 轮次（1-based）。
    pub round: usize,
    /// 本轮错误数。
    pub errors: usize,
    /// 本轮警告数。
    pub warnings: usize,
    /// 本轮验证耗时（毫秒）。
    pub duration_ms: u128,
    /// 本轮之后触发的修复动作（最后一轮或已通过时为 `None`）。
    pub fix: Option<FixAttempt>,
}

/// 验证-修复循环的汇总。
#[derive(Debug, Clone)]
pub struct VerifyFixReport {
    /// 每轮记录。
    pub rounds: Vec<VerifyRound>,
    /// 是否达到门禁。
    pub passed: bool,
    /// 实际使用的轮数上限。
    pub max_rounds: usize,
    /// 总耗时（毫秒）。
    pub total_duration_ms: u128,
    /// 最后一轮的诊断报告（无轮次时为 `None`，正常不会发生）。
    pub last_report: Option<DiagnoseReport>,
}

impl VerifyFixReport {
    /// 实际执行的轮数。
    pub fn rounds_used(&self) -> usize {
        self.rounds.len()
    }

    /// 是否执行过修复动作。
    pub fn fix_attempts(&self) -> usize {
        self.rounds.iter().filter(|r| r.fix.is_some()).count()
    }
}

/// 有界验证-修复循环：`verify → fix → verify ...`。
///
/// - 每轮先诊断（check，check 通过时含 lint）；达到门禁即结束。
/// - 未通过且仍有剩余轮次时执行一次 `backend.fix()`，再进入下一轮。
/// - 修复失败不会被吞掉：记录在 `FixAttempt.message`，循环继续到上限。
/// - 轮数上限保证终止；不会无限重试。
pub fn verify_fix_loop_with(
    backend: &dyn LanguageBackend,
    options: &VerifyFixOptions<'_>,
    cwd: Option<&Path>,
) -> ToolResult<VerifyFixReport> {
    let limit = options.rounds();
    let started = std::time::Instant::now();
    let mut rounds = Vec::new();
    let mut last_report: Option<DiagnoseReport> = None;
    let mut passed = false;

    for round in 1..=limit {
        let round_started = std::time::Instant::now();
        let report = diagnose_with(backend, options.pkg, options.all, options.no_lint, cwd)?;
        let errors = report.total_errors;
        let warnings = report.total_warnings;
        let clean = options.is_clean(&report);
        last_report = Some(report);

        let mut record = VerifyRound {
            round,
            errors,
            warnings,
            duration_ms: round_started.elapsed().as_millis(),
            fix: None,
        };

        if clean {
            passed = true;
            rounds.push(record);
            break;
        }
        if round == limit {
            rounds.push(record);
            break;
        }

        let fix_started = std::time::Instant::now();
        let fix = match backend.fix(options.pkg, options.fix_lint, cwd) {
            Ok(output) => FixAttempt {
                success: output.success,
                errors: output.errors.len(),
                duration_ms: fix_started.elapsed().as_millis(),
                message: None,
            },
            Err(err) => FixAttempt {
                success: false,
                errors: 0,
                duration_ms: fix_started.elapsed().as_millis(),
                message: Some(err.to_string()),
            },
        };
        record.fix = Some(fix);
        rounds.push(record);
    }

    Ok(VerifyFixReport {
        rounds,
        passed,
        max_rounds: limit,
        total_duration_ms: started.elapsed().as_millis(),
        last_report,
    })
}

/// Backward-compatible: runs fix+diagnose with RustBackend.
pub fn fix_and_diagnose(
    pkg: Option<&str>,
    fix_clippy: bool,
    all: bool,
    cwd: Option<&Path>,
) -> ToolResult<DiagnoseReport> {
    fix_and_diagnose_with(&build::rust::RustBackend, pkg, fix_clippy, all, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_toml(dir: &Path, content: &str) {
        // Ensure standalone project by appending [workspace] to avoid parent workspace interference
        let toml = if content.contains("[workspace]") {
            content.to_string()
        } else {
            format!("{content}\n[workspace]\n")
        };
        fs::write(dir.join("Cargo.toml"), toml).unwrap();
    }

    fn create_simple_project() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        write_toml(
            &path,
            r#"[package]
name = "diag-test"
version = "0.1.0"
edition = "2021"
"#,
        );
        let src = path.join("src");
        fs::create_dir_all(&src).unwrap();
        (dir, path)
    }

    #[test]
    fn test_diagnose_clean_project() {
        let (_d, path) = create_simple_project();
        fs::write(path.join("src/lib.rs"), "pub fn ok() -> u8 { 42 }\n").unwrap();
        let report = diagnose(None, false, true, Some(&path)).unwrap();
        assert!(report.check_output.success);
        assert_eq!(report.total_errors, 0);
        assert_eq!(report.total_warnings, 0);
    }

    #[test]
    fn test_diagnose_with_errors() {
        let (_d, path) = create_simple_project();
        fs::write(
            path.join("src/lib.rs"),
            "pub fn bad() -> u8 { nonexistent }\n",
        )
        .unwrap();
        let report = diagnose(None, false, true, Some(&path)).unwrap();
        assert!(!report.check_output.success);
        assert!(report.total_errors > 0);
    }

    #[test]
    fn test_diagnose_with_clippy() {
        let (_d, path) = create_simple_project();
        fs::write(
            path.join("src/lib.rs"),
            "pub fn add(x: u8, y: u8) -> u8 { x + y }\n",
        )
        .unwrap();
        let report = diagnose(None, false, false, Some(&path)).unwrap();
        if report.check_output.success {
            assert!(report.clippy_output.is_some());
            assert!(report.lint_output.is_some());
        } else {
            assert!(report.clippy_output.is_none());
            assert!(report.lint_output.is_none());
        }
    }

    #[test]
    fn test_diagnose_no_clippy_flag() {
        let (_d, path) = create_simple_project();
        fs::write(path.join("src/lib.rs"), "pub fn ok() -> u8 { 42 }\n").unwrap();
        let report = diagnose(None, false, false, Some(&path)).unwrap();
        assert!(report.check_output.success);
    }

    #[test]
    fn test_fix_and_diagnose() {
        let (_d, path) = create_simple_project();
        fs::write(path.join("src/lib.rs"), "pub fn test() { let x = 1; }\n").unwrap();
        let report = fix_and_diagnose(None, false, false, Some(&path)).unwrap();
        assert!(report.check_output.success);
    }

    #[test]
    fn test_lint_fix_path_applies_clippy_suggestions() {
        // 回归：`cargo fix --clippy` 不是合法参数，lint 修复必须走
        // `cargo clippy --fix`。旧实现让 `--lint-fixes` 的修复步骤必然失败
        // （cargo 报 "unexpected argument '--clippy'"）。
        let (_d, path) = create_simple_project();
        fs::write(
            path.join("src/lib.rs"),
            "pub fn value() -> i32 {\n    return 42;\n}\n",
        )
        .unwrap();

        let report = fix_and_diagnose(None, true, false, Some(&path)).unwrap();
        assert!(report.check_output.success, "修复后 check 应成功");

        let after = fs::read_to_string(path.join("src/lib.rs")).unwrap();
        assert!(
            !after.contains("return 42"),
            "clippy 的 needless_return 应被自动修复，实际内容: {after}"
        );
    }

    #[test]
    fn test_diagnose_groups_by_file() {
        let (_d, path) = create_simple_project();
        fs::write(
            path.join("src/lib.rs"),
            "pub fn bad() -> u8 { nonexistent }\n",
        )
        .unwrap();
        let report = diagnose(None, false, true, Some(&path)).unwrap();
        assert!(!report.errors_by_file.is_empty());
        let (file, count) = &report.errors_by_file[0];
        assert!(file.contains("lib.rs"));
        assert!(*count > 0);
    }

    #[test]
    fn test_diagnose_all_diagnostics_sorted() {
        let (_d, path) = create_simple_project();
        fs::write(
            path.join("src/lib.rs"),
            "pub fn bad() -> u8 { nonexistent }\n",
        )
        .unwrap();
        let report = diagnose(None, false, true, Some(&path)).unwrap();
        for pair in report.all_diagnostics.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            let a_key = (a.file.as_deref().unwrap_or(""), a.line.unwrap_or(0));
            let b_key = (b.file.as_deref().unwrap_or(""), b.line.unwrap_or(0));
            assert!(a_key <= b_key);
        }
    }

    #[test]
    fn test_diagnose_with_rustbackend() {
        let (_d, path) = create_simple_project();
        fs::write(path.join("src/lib.rs"), "pub fn ok() -> u8 { 42 }\n").unwrap();
        let report =
            diagnose_with(&build::rust::RustBackend, None, false, true, Some(&path)).unwrap();
        assert!(report.check_output.success);
        assert_eq!(report.total_errors, 0);
    }

    #[test]
    fn test_diagnose_auto_detects_rust() {
        let (_d, path) = create_simple_project();
        fs::write(path.join("src/lib.rs"), "pub fn ok() -> u8 { 42 }\n").unwrap();
        let report = diagnose_auto(None, false, true, Some(&path)).unwrap();
        assert!(report.check_output.success);
    }

    // ── 验证-修复循环 ────────────────────────────────────────

    use sisyphus_tools_build::build::TestResults;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// 可编排的假后端：按脚本返回 (errors, warnings)，并统计 fix 调用次数。
    #[derive(Debug)]
    struct ScriptedBackend {
        script: Mutex<VecDeque<(usize, usize)>>,
        fix_calls: Mutex<usize>,
        fix_output_errors: usize,
    }

    impl ScriptedBackend {
        fn new(script: Vec<(usize, usize)>, fix_output_errors: usize) -> Self {
            Self {
                script: Mutex::new(script.into()),
                fix_calls: Mutex::new(0),
                fix_output_errors,
            }
        }

        fn fix_calls(&self) -> usize {
            *self.fix_calls.lock().expect("lock")
        }
    }

    fn diags(level: &str, count: usize) -> Vec<Diag> {
        (0..count)
            .map(|i| Diag {
                file: Some("src/lib.rs".to_string()),
                line: Some(i + 1),
                col: Some(1),
                level: level.to_string(),
                code: None,
                message: format!("{level} #{i}"),
            })
            .collect()
    }

    impl LanguageBackend for ScriptedBackend {
        fn name(&self) -> &str {
            "scripted"
        }

        fn check(
            &self,
            _pkg: Option<&str>,
            _all: bool,
            _cwd: Option<&Path>,
        ) -> ToolResult<BuildOutput> {
            let (errors, warnings) =
                self.script.lock().expect("lock").pop_front().unwrap_or((0, 0));
            Ok(BuildOutput {
                errors: diags("error", errors),
                warnings: diags("warning", warnings),
                raw_stdout: String::new(),
                raw_stderr: String::new(),
                success: errors == 0,
            })
        }

        fn build(
            &self,
            _pkg: Option<&str>,
            _release: bool,
            _cwd: Option<&Path>,
        ) -> ToolResult<BuildOutput> {
            Ok(BuildOutput {
                errors: Vec::new(),
                warnings: Vec::new(),
                raw_stdout: String::new(),
                raw_stderr: String::new(),
                success: true,
            })
        }

        fn test(
            &self,
            _pkg: Option<&str>,
            _name: Option<&str>,
            _all: bool,
            _cwd: Option<&Path>,
        ) -> ToolResult<TestResults> {
            Ok(TestResults {
                passed: 0,
                failed: 0,
                ignored: 0,
                measured: 0,
                filtered_out: 0,
                failures: Vec::new(),
                raw_stdout: String::new(),
                raw_stderr: String::new(),
            })
        }

        fn lint(&self, _pkg: Option<&str>, _cwd: Option<&Path>) -> ToolResult<BuildOutput> {
            Ok(BuildOutput {
                errors: Vec::new(),
                warnings: Vec::new(),
                raw_stdout: String::new(),
                raw_stderr: String::new(),
                success: true,
            })
        }

        fn fix(
            &self,
            _pkg: Option<&str>,
            _apply_lint_fixes: bool,
            _cwd: Option<&Path>,
        ) -> ToolResult<BuildOutput> {
            *self.fix_calls.lock().expect("lock") += 1;
            Ok(BuildOutput {
                errors: diags("error", self.fix_output_errors),
                warnings: Vec::new(),
                raw_stdout: String::new(),
                raw_stderr: String::new(),
                success: true,
            })
        }

        fn fmt_check(&self, _cwd: Option<&Path>) -> ToolResult<bool> {
            Ok(true)
        }

        fn fmt_write(&self, _cwd: Option<&Path>) -> ToolResult<()> {
            Ok(())
        }

        fn doc(&self, _pkg: Option<&str>, _cwd: Option<&Path>) -> ToolResult<String> {
            Ok(String::new())
        }
    }

    fn opts(max_rounds: usize, max_warnings: Option<usize>) -> VerifyFixOptions<'static> {
        VerifyFixOptions {
            pkg: None,
            all: false,
            no_lint: true,
            fix_lint: false,
            max_warnings,
            max_rounds,
        }
    }

    #[test]
    fn test_verify_loop_clean_first_round_skips_fix() {
        let backend = ScriptedBackend::new(vec![(0, 0)], 0);
        let report = verify_fix_loop_with(&backend, &opts(3, None), None).unwrap();
        assert!(report.passed);
        assert_eq!(report.rounds_used(), 1);
        assert_eq!(report.fix_attempts(), 0);
        assert_eq!(backend.fix_calls(), 0);
    }

    #[test]
    fn test_verify_loop_fixes_then_passes() {
        let backend = ScriptedBackend::new(vec![(2, 0), (0, 0)], 0);
        let report = verify_fix_loop_with(&backend, &opts(3, None), None).unwrap();
        assert!(report.passed);
        assert_eq!(report.rounds_used(), 2);
        assert_eq!(report.fix_attempts(), 1);
        assert_eq!(backend.fix_calls(), 1);
        assert_eq!(report.rounds[0].errors, 2);
        assert!(report.rounds[0].fix.as_ref().expect("fix").success);
        assert_eq!(report.rounds[1].errors, 0);
        assert!(report.rounds[1].fix.is_none());
    }

    #[test]
    fn test_verify_loop_is_bounded_on_unfixable_errors() {
        let backend = ScriptedBackend::new(vec![(1, 0), (1, 0), (1, 0), (1, 0)], 1);
        let report = verify_fix_loop_with(&backend, &opts(3, None), None).unwrap();
        assert!(!report.passed);
        assert_eq!(report.rounds_used(), 3);
        assert_eq!(report.fix_attempts(), 2, "最后一轮不再尝试修复");
        assert_eq!(backend.fix_calls(), 2);
        assert_eq!(report.max_rounds, 3);
    }

    #[test]
    fn test_verify_loop_zero_rounds_means_one() {
        let backend = ScriptedBackend::new(vec![(1, 0)], 0);
        let report = verify_fix_loop_with(&backend, &opts(0, None), None).unwrap();
        assert_eq!(report.max_rounds, 1);
        assert_eq!(report.rounds_used(), 1);
        assert!(!report.passed);
    }

    #[test]
    fn test_verify_loop_max_warnings_gate() {
        // 第 1 轮：无错误但有 1 个警告（零警告门禁下未通过）→ fix → 第 2 轮 0 警告
        let backend = ScriptedBackend::new(vec![(0, 1), (0, 0)], 0);
        let strict = verify_fix_loop_with(&backend, &opts(3, Some(0)), None).unwrap();
        assert!(strict.passed);
        assert_eq!(strict.rounds_used(), 2);
        assert_eq!(strict.fix_attempts(), 1);

        // 未设置警告门禁时，第 1 轮即通过，不触发 fix
        let backend2 = ScriptedBackend::new(vec![(0, 1)], 0);
        let lenient = verify_fix_loop_with(&backend2, &opts(3, None), None).unwrap();
        assert!(lenient.passed);
        assert_eq!(lenient.rounds_used(), 1);
        assert_eq!(backend2.fix_calls(), 0);
    }
}
