use anyhow::Result;

use crate::commands::*;

pub fn handle_fix(sub: &FixCommands) -> Result<()> {
    match sub {
        FixCommands::Run {
            lang,
            package,
            lint_fixes,
            all,
            cwd,
        } => sisyphus_cli::diagnose::run_fix(
            lang.clone(),
            package.clone(),
            *lint_fixes,
            *all,
            cwd.clone(),
        )?,
    }
    Ok(())
}
