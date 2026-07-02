use anyhow::Result;

use crate::commands::*;

pub fn handle_build(sub: &BuildCommands) -> Result<()> {
    match sub {
        BuildCommands::Check {
            package,
            all,
            lang,
            cwd,
        } => sisyphus_cli::build::run_check(package.clone(), *all, cwd.clone(), lang.clone())?,
        BuildCommands::Build {
            package,
            release,
            lang,
            cwd,
        } => sisyphus_cli::build::run_build(package.clone(), *release, cwd.clone(), lang.clone())?,
        BuildCommands::Test {
            package,
            name,
            all,
            lang,
            cwd,
        } => sisyphus_cli::build::run_test(
            package.clone(),
            name.clone(),
            *all,
            cwd.clone(),
            lang.clone(),
        )?,
        BuildCommands::Lint { package, lang, cwd } => {
            sisyphus_cli::build::run_lint(package.clone(), cwd.clone(), lang.clone())?
        }
        BuildCommands::Fmt { check, lang, cwd } => {
            sisyphus_cli::build::run_fmt(*check, cwd.clone(), lang.clone())?
        }
        BuildCommands::Doc { package, lang, cwd } => {
            sisyphus_cli::build::run_doc(package.clone(), cwd.clone(), lang.clone())?
        }
    }
    Ok(())
}
