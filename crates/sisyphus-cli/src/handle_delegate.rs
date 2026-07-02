use anyhow::Result;

use crate::commands::*;

pub async fn handle_delegate(sub: &DelegateCommands) -> Result<()> {
    match sub {
        DelegateCommands::Run {
            file,
            max_concurrent,
        } => {
            sisyphus_cli::delegate::run_tasks(file.clone(), *max_concurrent).await?;
        }
        DelegateCommands::Inline {
            commands,
            max_concurrent,
        } => {
            sisyphus_cli::delegate::run_inline(commands, *max_concurrent).await?;
        }
    }
    Ok(())
}
