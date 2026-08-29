use anyhow::Result;

use crate::commands::*;

/// Handle plugin.
pub async fn handle_plugin(sub: &PluginCommands, bridge_addr: &str) -> Result<()> {
    let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
    match sub {
        PluginCommands::List => {
            sisyphus_cli::plugin::run_list(&mut client).await?;
        }
        PluginCommands::Load { path } => {
            sisyphus_cli::plugin::run_load(&mut client, path).await?;
        }
        PluginCommands::Watch { path } => {
            sisyphus_cli::plugin::run_watch(path.as_deref()).await?;
        }
        PluginCommands::Unload { name } => {
            sisyphus_cli::plugin::run_unload(&mut client, name).await?;
        }
    }
    Ok(())
}
