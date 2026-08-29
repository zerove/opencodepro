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
        }
        Some("node") | Some("ts") | Some("js") => {
            Ok(Box::new(crate::tools::build::node::NodeBackend::new()?))
        }
        Some("python") | Some("py") => {
            Ok(Box::new(crate::tools::build::python::PythonBackend::new()?))
        }
        Some("go") | Some("golang") => Ok(Box::new(crate::tools::build::golang::GoBackend)),
        Some("java") | Some("maven") => {
            Ok(Box::new(crate::tools::build::java::JavaBackend::new_maven()))
        }
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
            let dir = cwd.unwrap_or(std::path::Path::new("."));
            Ok(crate::tools::build::detect_backend(dir)?)
        }
    }
}

/// Run run.
pub fn run_run(args: Vec<String>, lang: Option<String>, cwd: Option<PathBuf>) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd.as_deref();
    let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let output = backend.run(&str_args, dir_ref)?;
    print!("{output}");
    Ok(())
}
