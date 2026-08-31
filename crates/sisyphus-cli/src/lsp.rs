use anyhow::{Context, Result};

async fn spawn_client(
    server_cmd: &str,
    server_args: &[String],
    root_uri: &str,
) -> Result<crate::tools::lsp::LspClient> {
    let args_refs: Vec<&str> = server_args.iter().map(|s| s.as_str()).collect();
    let client = crate::tools::lsp::LspClient::spawn(server_cmd, &args_refs, root_uri)
        .await
        .context("Failed to spawn language server")?;
    Ok(client)
}

/// Run diagnostics.
pub async fn run_diagnostics(
    file: &std::path::Path,
    language_id: &str,
    server_cmd: &str,
    server_args: &[String],
    root_uri: &str,
) -> Result<()> {
    let diags = run_diagnostics_raw(file, language_id, server_cmd, server_args, root_uri).await?;
    if diags.is_empty() {
        println!("No diagnostics.");
    } else {
        for d in &diags {
            let sev = d.severity.map(crate::tools::lsp::severity_name).unwrap_or("?");
            let r = &d.range;
            println!(
                "  {}:{}  {}  {}",
                r.start.line, r.start.character, sev, d.message
            );
        }
    }
    Ok(())
}

/// 运行 LSP 诊断并返回原始结果（供 TUI 使用）。
pub async fn run_diagnostics_raw(
    file: &std::path::Path,
    language_id: &str,
    server_cmd: &str,
    server_args: &[String],
    root_uri: &str,
) -> Result<Vec<crate::tools::lsp::LspDiagnostic>> {
    let mut client = spawn_client(server_cmd, server_args, root_uri)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to spawn LSP server: {e}"))?;
    client
        .open_document(&file.to_string_lossy(), language_id)
        .await
        .context("Failed to open document")?;

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let diags = client.get_diagnostics(&file.to_string_lossy()).await.map_err(|e| {
        let stderr = client.take_stderr();
        if stderr.is_empty() {
            anyhow::anyhow!("Failed to get diagnostics: {e}")
        } else {
            anyhow::anyhow!("Failed to get diagnostics: {e}\nServer stderr:\n{stderr}")
        }
    })?;

    client.shutdown().await.ok();
    Ok(diags)
}

/// Run goto def.
pub async fn run_goto_def(
    file: &std::path::Path,
    line: u32,
    col: u32,
    language_id: &str,
    server_cmd: &str,
    server_args: &[String],
    root_uri: &str,
) -> Result<()> {
    let mut client = spawn_client(server_cmd, server_args, root_uri).await?;
    client
        .open_document(&file.to_string_lossy(), language_id)
        .await
        .context("Failed to open document")?;

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    match client
        .goto_definition(&file.to_string_lossy(), line, col)
        .await
        .context("goto-definition failed")?
    {
        Some(loc) => println!(
            "{}:{}:{}",
            loc.uri, loc.range.start.line, loc.range.start.character
        ),
        None => println!("No definition found."),
    }
    client.shutdown().await.ok();
    Ok(())
}

/// Run find refs.
pub async fn run_find_refs(
    file: &std::path::Path,
    line: u32,
    col: u32,
    language_id: &str,
    server_cmd: &str,
    server_args: &[String],
    root_uri: &str,
) -> Result<()> {
    let mut client = spawn_client(server_cmd, server_args, root_uri).await?;
    client
        .open_document(&file.to_string_lossy(), language_id)
        .await
        .context("Failed to open document")?;

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let refs = client
        .find_references(&file.to_string_lossy(), line, col)
        .await
        .context("find-references failed")?;

    if refs.is_empty() {
        println!("No references found.");
    } else {
        for loc in &refs {
            println!(
                "{}:{}:{}",
                loc.uri, loc.range.start.line, loc.range.start.character
            );
        }
    }
    client.shutdown().await.ok();
    Ok(())
}

/// Run hover.
pub async fn run_hover(
    file: &std::path::Path,
    line: u32,
    col: u32,
    language_id: &str,
    server_cmd: &str,
    server_args: &[String],
    root_uri: &str,
) -> Result<()> {
    let mut client = spawn_client(server_cmd, server_args, root_uri).await?;
    client
        .open_document(&file.to_string_lossy(), language_id)
        .await
        .context("Failed to open document")?;

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    match client.get_hover(&file.to_string_lossy(), line, col).await.context("hover failed")? {
        Some(hover) => println!("{}", hover.contents),
        None => println!("No hover information."),
    }
    client.shutdown().await.ok();
    Ok(())
}

/// Run symbols.
pub async fn run_symbols(
    file: &std::path::Path,
    language_id: &str,
    server_cmd: &str,
    server_args: &[String],
    root_uri: &str,
) -> Result<()> {
    let mut client = spawn_client(server_cmd, server_args, root_uri).await?;
    client
        .open_document(&file.to_string_lossy(), language_id)
        .await
        .context("Failed to open document")?;

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let symbols =
        client.get_symbols(&file.to_string_lossy()).await.context("get-symbols failed")?;

    if symbols.is_empty() {
        println!("No symbols found.");
    } else {
        for s in &symbols {
            println!(
                "{}:{}  kind={}  {}",
                s.range.start.line, s.range.start.character, s.kind, s.name
            );
        }
    }
    client.shutdown().await.ok();
    Ok(())
}
