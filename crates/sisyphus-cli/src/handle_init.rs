use anyhow::Result;

use crate::commands::*;

/// Handle init.
pub fn handle_init(sub: &InitCommands) -> Result<()> {
    match sub {
        InitCommands::List => sisyphus_cli::init::run_list()?,
        InitCommands::Generate {
            template,
            name,
            dir,
        } => sisyphus_cli::init::run_generate(template, name, dir)?,
    }
    Ok(())
}
