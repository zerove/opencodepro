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
        let existing: Vec<(Option<String>, Option<usize>, String)> = all_diags
            .iter()
            .map(|d| (d.file.clone(), d.line, d.message.clone()))
            .collect();
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
}
