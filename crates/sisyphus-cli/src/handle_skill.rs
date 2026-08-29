use anyhow::Result;

use crate::commands::*;

/// Handle skill.
pub fn handle_skill(sub: &SkillCommands) -> Result<()> {
    match sub {
        SkillCommands::List => {
            sisyphus_cli::skill::run_list()?;
        }
        SkillCommands::Show { name } => {
            sisyphus_cli::skill::run_show(name)?;
        }
        SkillCommands::Install { path } => {
            sisyphus_cli::skill::run_install(path)?;
        }
        SkillCommands::Remove { name } => {
            sisyphus_cli::skill::run_remove(name)?;
        }
    }
    Ok(())
}
