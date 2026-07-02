use anyhow::Result;
use std::path::PathBuf;

/// Resolve language backend from `--lang` option string.
fn resolve_backend(
    lang: Option<&str>,
    cwd: Option<&std::path::Path>,
) -> Result<Box<dyn crate::tools::build::LanguageBackend>> {
    match lang {
        Some("rust") | None
            if cwd.map(|d| d.join("Cargo.toml").exists()).unwrap_or(false)
                || lang == Some("rust") =>
        {
            Ok(Box::new(crate::tools::build::rust::RustBackend))
        }
        Some("node") | Some("ts") | Some("js") => {
            Ok(Box::new(crate::tools::build::node::NodeBackend::new()?))
        }
        Some("python") | Some("py") => Ok(Box::new(
            crate::tools::build::python::PythonBackend::new()?,
        )),
        Some("go") | Some("golang") => Ok(Box::new(crate::tools::build::golang::GoBackend)),
        Some("java") | Some("maven") => Ok(Box::new(
            crate::tools::build::java::JavaBackend::new_maven(),
        )),
        Some("gradle") => Ok(Box::new(
            crate::tools::build::java::JavaBackend::new_gradle(),
        )),
        Some("fe") | Some("frontend") | Some("css") | Some("html") => {
            Ok(Box::new(crate::tools::build::frontend::FrontendBackend))
        }
        Some(other) => {
            anyhow::bail!("Unknown language backend: {other}. Supported: rust, node/ts/js, python/py, go/golang, java/maven, gradle, fe/frontend/css/html")
        }
        None => {
            // Auto-detect
            let dir = cwd.unwrap_or(std::path::Path::new("."));
            Ok(crate::tools::build::detect_backend(dir)?)
        }
    }
}

pub fn run_diagnose(
    lang: Option<String>,
    package: Option<String>,
    all: bool,
    no_lint: bool,
    cwd: Option<PathBuf>,
) -> Result<()> {
    let dir = cwd.as_deref();
    let pkg = package.as_deref();
    let backend = resolve_backend(lang.as_deref(), dir)?;

    let report = crate::tools::diagnose_with(backend.as_ref(), pkg, all, no_lint, dir)?;

    println!("=== Diagnosis Report (backend: {}) ===", backend.name());
    if report.check_output.success {
        println!("check: ✅ PASSED");
    } else {
        println!(
            "check: ❌ FAILED ({} errors, {} warnings)",
            report.check_output.errors.len(),
            report.check_output.warnings.len()
        );
    }

    if let Some(ref lint) = report.lint_output {
        if lint.success {
            println!("lint:  ✅ PASSED ({} warnings)", lint.warnings.len());
        } else {
            println!(
                "lint:  ❌ ISSUES FOUND ({} errors, {} warnings)",
                lint.errors.len(),
                lint.warnings.len()
            );
        }
    } else if !no_lint && !report.check_output.success {
        println!("lint:  ⏭ SKIPPED (check failed)");
    } else if no_lint {
        println!("lint:  ⏭ SKIPPED (--no-lint)");
    }

    println!();
    println!("Total errors:   {}", report.total_errors);
    println!("Total warnings: {}", report.total_warnings);

    if !report.errors_by_file.is_empty() {
        println!();
        println!("Errors by file:");
        for (file, count) in &report.errors_by_file {
            println!("  {}: {}", file, count);
        }
    }
    if !report.warnings_by_file.is_empty() {
        println!();
        println!("Warnings by file:");
        for (file, count) in &report.warnings_by_file {
            println!("  {}: {}", file, count);
        }
    }

    if !report.all_diagnostics.is_empty() {
        println!();
        let show = report.all_diagnostics.len().min(20);
        println!(
            "Diagnostics (showing first {}/{}):",
            show,
            report.all_diagnostics.len()
        );
        for d in report.all_diagnostics.iter().take(show) {
            let loc = match (&d.file, d.line) {
                (Some(f), Some(ln)) => format!("{}:{}", f, ln),
                (Some(f), None) => f.clone(),
                (None, _) => "<unknown>".to_string(),
            };
            let code = d
                .code
                .as_deref()
                .map(|c| format!("[{c}]"))
                .unwrap_or_default();
            println!("  {} {} {} — {}", d.level, code, loc, d.message);
        }
        if report.all_diagnostics.len() > 20 {
            println!("  ... and {} more", report.all_diagnostics.len() - 20);
        }
    }

    if report.total_errors == 0 && report.total_warnings == 0 {
        println!();
        println!("✅ No issues found.");
    }

    Ok(())
}

pub fn run_fix(
    lang: Option<String>,
    package: Option<String>,
    fix_lint: bool,
    all: bool,
    cwd: Option<PathBuf>,
) -> Result<()> {
    let dir = cwd.as_deref();
    let pkg = package.as_deref();
    let backend = resolve_backend(lang.as_deref(), dir)?;

    println!(
        "Running fix (backend: {}){}...",
        backend.name(),
        if fix_lint { " + lint fixes" } else { "" }
    );
    let report = crate::tools::fix_and_diagnose_with(backend.as_ref(), pkg, fix_lint, all, dir)?;

    if report.check_output.success {
        println!("✅ Fix completed successfully.");
    } else {
        println!("❌ Fix completed with remaining issues.");
    }

    if report.total_errors > 0 || report.total_warnings > 0 {
        println!();
        println!("Remaining after fix:");
        println!("  Errors:   {}", report.total_errors);
        println!("  Warnings: {}", report.total_warnings);

        if !report.errors_by_file.is_empty() {
            println!("  Errors by file:");
            for (file, count) in &report.errors_by_file {
                println!("    {}: {}", file, count);
            }
        }
    } else {
        println!("✅ All clean after fix.");
    }

    Ok(())
}
