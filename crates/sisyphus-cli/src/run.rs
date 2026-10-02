use crate::build::resolve_backend;
use anyhow::Result;
use std::path::PathBuf;

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
