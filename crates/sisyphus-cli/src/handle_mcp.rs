use std::sync::Arc;

use anyhow::{Context, Result};

use crate::commands::*;

/// Handle mcp.
pub async fn handle_mcp(
    sub: &McpCommands,
    llm_client: Arc<dyn sisyphus_cli::tools::metis::LlmClient>,
) -> Result<()> {
    match sub {
        McpCommands::Info { connect } => {
            sisyphus_cli::mcp::run_info(connect)
                .await
                .with_context(|| {
                    format!(
                        "MCP info 失败。请检查服务器命令/URL 与参数是否正确（connect={connect:?}）。\
提示: stdio 用 `<command> [args...]`，HTTP 用 `http(s)://...`；详见 docs/runbooks/single-node-tools-mcp-guide.md"
                    )
                })?;
        },
        McpCommands::Call {
            tool,
            args,
            connect,
        } => {
            sisyphus_cli::mcp::run_call(tool, args.as_deref(), connect)
                .await
                .with_context(|| {
                    format!(
                        "MCP call 失败（tool={tool}）。请先用 `sisyphus mcp info -- <connect...>` 确认可用工具与参数；\
检查连接、鉴权与工具参数 JSON。详见 docs/runbooks/single-node-tools-mcp-guide.md"
                    )
                })?;
        },
        McpCommands::Serve => {
            sisyphus_cli::mcp::run_serve(llm_client.clone()).await.context(
                "MCP serve 启动失败。请检查本地环境与依赖；该模式通过 stdio 暴露代理工具。",
            )?;
        },
        McpCommands::Resources { connect } => {
            sisyphus_cli::mcp::run_resources(connect).await.with_context(|| {
                format!(
                    "MCP resources 失败。请检查服务器命令/URL 与参数是否正确（connect={connect:?}）。"
                )
            })?;
        },
        McpCommands::ReadResource { uri, connect } => {
            sisyphus_cli::mcp::run_read_resource(uri, connect)
                .await
                .with_context(|| format!("MCP read-resource 失败（uri={uri}）。"))?;
        },
        McpCommands::Prompts { connect } => {
            sisyphus_cli::mcp::run_prompts(connect).await.with_context(|| {
                format!(
                    "MCP prompts 失败。请检查服务器命令/URL 与参数是否正确（connect={connect:?}）。"
                )
            })?;
        },
        McpCommands::GetPrompt {
            name,
            args,
            connect,
        } => {
            sisyphus_cli::mcp::run_get_prompt(name, args.as_deref(), connect)
                .await
                .with_context(|| format!("MCP get-prompt 失败（name={name}）。"))?;
        },
    }
    Ok(())
}
