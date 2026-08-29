use anyhow::Result;

use crate::commands::*;

/// Handle sop.
pub async fn handle_sop(sub: &SopCommands, bridge_addr: &str) -> Result<()> {
    match sub {
        SopCommands::Run { sop_id } => {
            let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
            let result = client.execute_sop(sop_id).await?;
            println!("{result}");
        }
        SopCommands::List => {
            let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
            let sops = client.list_sops().await?;
            if sops.is_empty() {
                println!("No SOPs registered.");
            } else {
                for sop in &sops {
                    println!("  {} ({} nodes)", sop.sop_id, sop.node_count);
                }
            }
        }
    }
    Ok(())
}
