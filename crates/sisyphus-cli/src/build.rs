use anyhow::Result;
use std::path::PathBuf;

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
        },
        Some("node") | Some("ts") | Some("js") => {
            Ok(Box::new(crate::tools::build::node::NodeBackend::new()?))
        },
        Some("python") | Some("py") => {
            Ok(Box::new(crate::tools::build::python::PythonBackend::new()?))
        },
        Some("go") | Some("golang") => Ok(Box::new(crate::tools::build::golang::GoBackend)),
        Some("java") | Some("maven") => {
            Ok(Box::new(crate::tools::build::java::JavaBackend::new_maven()))
        },
        Some("gradle") => Ok(Box::new(
            crate::tools::build::java::JavaBackend::new_gradle(),
        )),
        Some("fe") | Some("frontend") | Some("css") | Some("html") => {
            Ok(Box::new(crate::tools::build::frontend::FrontendBackend))
        },
        Some(other) => {
            anyhow::bail!("Unknown language backend: {other}. Supported: rust, node/ts/js, python/py, go/golang, java/maven, gradle, fe/frontend/css/html")
        },
        None => {
            let dir = cwd.unwrap_or(std::path::Path::new("."));
            Ok(crate::tools::build::detect_backend(dir)?)
        },
    }
}

fn cwd_or_current(cwd: Option<PathBuf>) -> Option<PathBuf> {
    Some(cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_default()))
}

fn print_diags(label: &str, diags: &[crate::tools::build::Diag]) {
    if diags.is_empty() {
        return;
    }
    println!("  {label}:");
    for d in diags {
        let loc = match (&d.file, d.line) {
            (Some(f), Some(l)) => format!("{f}:{l}"),
            (Some(f), None) => f.clone(),
            _ => String::new(),
        };
        let code = d.code.as_deref().unwrap_or("");
        if loc.is_empty() {
            println!("    [{code}] {}", d.message);
        } else {
            println!("    {loc}  [{code}] {}", d.message);
        }
    }
}

/// Run check.
pub fn run_check(
    package: Option<String>,
    all: bool,
    cwd: Option<PathBuf>,
    lang: Option<String>,
) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    let out = backend.check(package.as_deref(), all, dir_ref.as_deref())?;
    if out.success {
        println!("Check passed (no errors).");
    } else {
        println!("Check FAILED:");
    }
    print_diags("Errors", &out.errors);
    print_diags("Warnings", &out.warnings);
    if !out.success {
        anyhow::bail!(
            "{} error(s), {} warning(s)",
            out.errors.len(),
            out.warnings.len()
        );
    }
    Ok(())
}

/// Run build.
pub fn run_build(
    package: Option<String>,
    release: bool,
    cwd: Option<PathBuf>,
    lang: Option<String>,
) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    let out = backend.build(package.as_deref(), release, dir_ref.as_deref())?;
    if out.success {
        println!("Build succeeded.");
    } else {
        println!("Build FAILED:");
    }
    print_diags("Errors", &out.errors);
    print_diags("Warnings", &out.warnings);
    if !out.success {
        anyhow::bail!(
            "{} error(s), {} warning(s)",
            out.errors.len(),
            out.warnings.len()
        );
    }
    Ok(())
}

/// Run test.
pub fn run_test(
    package: Option<String>,
    name: Option<String>,
    all: bool,
    cwd: Option<PathBuf>,
    lang: Option<String>,
) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    let r = backend.test(package.as_deref(), name.as_deref(), all, dir_ref.as_deref())?;

    let status = if r.failed == 0 { "ok" } else { "FAILED" };
    println!(
        "Test result: {status}. {} passed; {} failed; {} ignored; {} measured; {} filtered out",
        r.passed, r.failed, r.ignored, r.measured, r.filtered_out
    );

    if !r.failures.is_empty() {
        println!("\nFailures:");
        for f in &r.failures {
            println!("    {}", f.name);
        }
    }

    if r.failed > 0 {
        anyhow::bail!("{} test(s) failed", r.failed);
    }
    Ok(())
}

/// Run lint.
pub fn run_lint(package: Option<String>, cwd: Option<PathBuf>, lang: Option<String>) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    let out = backend.lint(package.as_deref(), dir_ref.as_deref())?;
    if out.success {
        println!("Lint passed (no warnings).");
    } else {
        println!("Lint issued warnings:");
    }
    print_diags("Warnings", &out.warnings);
    print_diags("Errors", &out.errors);
    Ok(())
}

/// Run fmt.
pub fn run_fmt(check_mode: bool, cwd: Option<PathBuf>, lang: Option<String>) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    if check_mode {
        let ok = backend.fmt_check(dir_ref.as_deref())?;
        if ok {
            println!("All files formatted correctly.");
        } else {
            anyhow::bail!("Some files are not formatted — run `sisyphus build fmt` to fix.");
        }
    } else {
        backend.fmt_write(dir_ref.as_deref())?;
        println!("Formatted all files.");
    }
    Ok(())
}

/// Run doc.
pub fn run_doc(package: Option<String>, cwd: Option<PathBuf>, lang: Option<String>) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    let out = backend.doc(package.as_deref(), dir_ref.as_deref())?;
    println!("{out}");
    Ok(())
}
