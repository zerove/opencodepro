use anyhow::Result;

use crate::commands::*;

pub async fn handle_health(bridge_addr: &str) -> Result<()> {
    let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
    let resp = client.health_check().await?;
    println!("Status: {}", resp.status);
    println!("Version: {}", resp.version);
    Ok(())
}

pub async fn handle_status(bridge_addr: &str) -> Result<()> {
    let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
    sisyphus_cli::remote::run_status(&mut client).await?;
    Ok(())
}

pub async fn handle_learning_insights(bridge_addr: &str, strategy: &Option<String>) -> Result<()> {
    let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
    sisyphus_cli::remote::run_learning_insights(&mut client, strategy.as_deref()).await?;
    Ok(())
}

pub async fn handle_remote(sub: &RemoteCommands, bridge_addr: &str) -> Result<()> {
    let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
    match sub {
        RemoteCommands::Bench => sisyphus_cli::remote::run_benchmarks(&mut client).await?,
        RemoteCommands::Agents => sisyphus_cli::remote::run_list_agents(&mut client).await?,
        RemoteCommands::Tools => sisyphus_cli::remote::run_list_tools(&mut client).await?,
        RemoteCommands::Skills => sisyphus_cli::remote::run_list_skills(&mut client).await?,
        RemoteCommands::Exec { sop_id } => {
            sisyphus_cli::remote::run_exec_sop(&mut client, sop_id).await?
        }
        RemoteCommands::Decompose { goal } => {
            sisyphus_cli::remote::run_decompose_goal(&mut client, goal).await?
        }
    }
    Ok(())
}
