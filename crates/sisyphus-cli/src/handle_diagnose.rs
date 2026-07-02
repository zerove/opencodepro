use anyhow::Result;

use crate::commands::*;

pub fn handle_diagnose(sub: &DiagnoseCommands) -> Result<()> {
    match sub {
        DiagnoseCommands::Run {
            lang,
            package,
            all,
            no_lint,
            cwd,
        } => sisyphus_cli::diagnose::run_diagnose(
            lang.clone(),
            package.clone(),
            *all,
            *no_lint,
            cwd.clone(),
        )?,
    }
    Ok(())
}
