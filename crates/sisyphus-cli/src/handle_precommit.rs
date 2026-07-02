use anyhow::Result;

use crate::commands::*;

pub fn handle_precommit(sub: &PrecommitCommands) -> Result<()> {
    match sub {
        PrecommitCommands::Install {
            dir,
            no_fmt,
            no_lint,
            allow_warnings,
            lang,
        } => sisyphus_cli::precommit::run_install(
            dir.clone(),
            *no_fmt,
            *no_lint,
            *allow_warnings,
            lang.clone(),
        )?,
        PrecommitCommands::Uninstall { dir } => {
            sisyphus_cli::precommit::run_uninstall(dir.clone())?
        }
        PrecommitCommands::Status { dir } => sisyphus_cli::precommit::run_status(dir.clone())?,
    }
    Ok(())
}
