use anyhow::Result;

use crate::commands::*;

/// Handle run.
pub fn handle_run(sub: &RunCommands) -> Result<()> {
    match sub {
        RunCommands::Run { args, lang, cwd } => {
            sisyphus_cli::run::run_run(args.clone(), lang.clone(), cwd.clone())?
        },
    }
    Ok(())
}
