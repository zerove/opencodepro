use std::sync::Arc;

use anyhow::Result;

use crate::commands::*;

pub async fn handle_mcp(
    sub: &McpCommands,
    llm_client: Arc<dyn sisyphus_cli::tools::metis::LlmClient>,
) -> Result<()> {
    match sub {
        McpCommands::Info { connect } => {
            sisyphus_cli::mcp::run_info(connect).await?;
        }
        McpCommands::Call {
            tool,
            args,
            connect,
        } => {
            sisyphus_cli::mcp::run_call(tool, args.as_deref(), connect).await?;
        }
        McpCommands::Serve => {
            sisyphus_cli::mcp::run_serve(llm_client.clone()).await?;
        }
    }
    Ok(())
}
