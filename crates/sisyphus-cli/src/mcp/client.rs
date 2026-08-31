use anyhow::{Context, Result};

// ── MCP 客户端 ────────────────────────────────────────────────

/// 辅助：通过 stdio 传输连接。
async fn connect_stdio(command: &str, args: &[String]) -> Result<crate::tools::McpClient> {
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let transport = crate::tools::StdioTransport::spawn(command, &args_refs)
        .context("Failed to spawn MCP server process")?;
    let mut client = crate::tools::McpClient::new(Box::new(transport));
    client.initialize("sisyphus-cli", "0.1.0").await.context("MCP initialize failed")?;
    Ok(client)
}

/// 辅助：通过 HTTP 传输连接。
async fn connect_http(url: &str) -> Result<crate::tools::McpClient> {
    let transport = crate::tools::HttpTransport::new(url);
    let mut client = crate::tools::McpClient::new(Box::new(transport));
    client.initialize("sisyphus-cli", "0.1.0").await.context("MCP initialize failed")?;
    Ok(client)
}

/// 解析连接参数 → 客户端。
/// 若第一个参数形如 `http://` 或 `https://`，使用 HTTP 传输。
/// 否则将参数视为 `<command>` `[args...]`，用于 stdio 传输。
async fn resolve_client(args: &[String]) -> Result<crate::tools::McpClient> {
    if args.is_empty() {
        anyhow::bail!("Provide either `<url>` for HTTP or `<command>` `[args...]` for stdio");
    }
    let first = &args[0];
    if first.starts_with("http://") || first.starts_with("https://") {
        connect_http(first).await
    } else {
        connect_stdio(first, &args[1..]).await
    }
}

/// `sisyphus mcp info <url-or-command...>` — 显示服务器信息 + 工具。
pub async fn run_info(connect_args: &[String]) -> Result<()> {
    let mut client = resolve_client(connect_args).await?;

    let info = client.server_info().cloned().unwrap_or(crate::tools::ServerInfo {
        name: "unknown".into(),
        version: "unknown".into(),
    });
    println!("Connected to MCP server: {} v{}", info.name, info.version);

    let tools = client.list_tools().await?;
    println!("Available tools ({}):", tools.len());
    for tool in &tools {
        let schema = tool
            .input_schema
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok())
            .unwrap_or_default();
        println!(
            "  {} — {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
        if !schema.is_empty() {
            println!("    schema: {schema}");
        }
    }
    Ok(())
}

/// `sisyphus mcp call <tool-name> [args-json] -- <url-or-command...>` — 调用工具。
pub async fn run_call(
    tool_name: &str,
    args_json: Option<&str>,
    connect_args: &[String],
) -> Result<()> {
    let mut client = resolve_client(connect_args).await?;

    let arguments = args_json
        .map(serde_json::from_str)
        .transpose()
        .context("Invalid JSON arguments — must be a valid JSON object")?;

    let result = client.call_tool(tool_name, arguments).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
