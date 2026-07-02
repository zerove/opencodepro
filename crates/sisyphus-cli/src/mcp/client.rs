use anyhow::{Context, Result};

// ── MCP Client ────────────────────────────────────────────────

/// Helper: connect via stdio transport.
async fn connect_stdio(command: &str, args: &[String]) -> Result<sisyphus_tools::McpClient> {
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let transport = sisyphus_tools::StdioTransport::spawn(command, &args_refs)
        .context("Failed to spawn MCP server process")?;
    let mut client = sisyphus_tools::McpClient::new(Box::new(transport));
    client
        .initialize("sisyphus-cli", "0.1.0")
        .await
        .context("MCP initialize failed")?;
    Ok(client)
}

/// Helper: connect via HTTP transport.
async fn connect_http(url: &str) -> Result<sisyphus_tools::McpClient> {
    let transport = sisyphus_tools::HttpTransport::new(url);
    let mut client = sisyphus_tools::McpClient::new(Box::new(transport));
    client
        .initialize("sisyphus-cli", "0.1.0")
        .await
        .context("MCP initialize failed")?;
    Ok(client)
}

/// Resolve connection args → client.
/// If first arg looks like `http://` or `https://`, uses HTTP transport.
/// Otherwise treats args as `<command>` `[args...]` for stdio transport.
async fn resolve_client(args: &[String]) -> Result<sisyphus_tools::McpClient> {
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

/// `sisyphus mcp info <url-or-command...>` — show server info + tools.
pub async fn run_info(connect_args: &[String]) -> Result<()> {
    let mut client = resolve_client(connect_args).await?;

    let info = client
        .server_info()
        .cloned()
        .unwrap_or(sisyphus_tools::ServerInfo {
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

/// `sisyphus mcp call <tool-name> [args-json] -- <url-or-command...>` — call a tool.
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
