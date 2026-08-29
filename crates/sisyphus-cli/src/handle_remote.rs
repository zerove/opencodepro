use anyhow::Result;

use crate::commands::*;

/// Handle health.
pub async fn handle_health(bridge_addr: &str) -> Result<()> {
    let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
    let resp = client.health_check().await?;
    println!("Status: {}", resp.status);
    println!("Version: {}", resp.version);
    Ok(())
}

/// Handle status.
pub async fn handle_status(bridge_addr: &str) -> Result<()> {
    let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
    sisyphus_cli::remote::run_status(&mut client).await?;
    Ok(())
}

/// Handle learning insights.
pub async fn handle_learning_insights(bridge_addr: &str, strategy: &Option<String>) -> Result<()> {
    let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
    sisyphus_cli::remote::run_learning_insights(&mut client, strategy.as_deref()).await?;
    Ok(())
}

/// Handle remote.
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

#[cfg(test)]
mod tests {
    use super::super::test_helpers::parse_commands;
    use crate::commands::{Commands, RemoteCommands};

    #[test]
    fn test_remote_commands_parse_agents() {
        let cmd = parse_commands(&["remote", "agents"]);
        assert!(matches!(cmd, Commands::Remote(RemoteCommands::Agents)));
    }

    #[test]
    fn test_remote_commands_parse_tools() {
        let cmd = parse_commands(&["remote", "tools"]);
        assert!(matches!(cmd, Commands::Remote(RemoteCommands::Tools)));
    }

    #[test]
    fn test_remote_commands_parse_exec() {
        let cmd = parse_commands(&["remote", "exec", "test-sop"]);
        match cmd {
            Commands::Remote(RemoteCommands::Exec { sop_id }) => {
                assert_eq!(sop_id, "test-sop");
            }
            _ => panic!("Expected Remote/Exec variant"),
        }
    }

    #[test]
    fn test_remote_commands_parse_decompose() {
        let cmd = parse_commands(&["remote", "decompose", "build a website"]);
        match cmd {
            Commands::Remote(RemoteCommands::Decompose { goal }) => {
                assert_eq!(goal, "build a website");
            }
            _ => panic!("Expected Remote/Decompose variant"),
        }
    }

    #[test]
    fn test_remote_parse_invalid() {
        use crate::Cli;
        use clap::Parser;
        assert!(Cli::try_parse_from(["sisyphus", "remote", "nonexistent"]).is_err());
    }
}
