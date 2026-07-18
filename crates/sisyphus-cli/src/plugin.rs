use crate::bridge::BridgeClient;
use anyhow::{Context, Result};
use std::path::Path;

pub async fn run_list(client: &mut BridgeClient) -> Result<()> {
    let plugins = client.list_plugins().await?;
    if plugins.is_empty() {
        println!("No plugins registered on the bridge.");
    } else {
        println!("Plugins registered on bridge:");
        for p in &plugins {
            let version_display = if p.version.is_empty() {
                "?".to_string()
            } else {
                p.version.clone()
            };
            println!(
                "  {} (v{}) - tool_id: {}, path: {}",
                p.name, version_display, p.tool_id, p.wasm_path
            );
        }
    }
    Ok(())
}

pub async fn run_load(client: &mut BridgeClient, path: &Path) -> Result<()> {
    let filename = path
        .file_name()
        .context("Invalid plugin path")?
        .to_string_lossy();
    let name = filename.trim_end_matches(".wasm");
    let wasm_path_str = path.to_string_lossy().to_string();

    let response = client
        .load_plugin(name, "1.0.0", &wasm_path_str, name, 0)
        .await?;
    if response.success {
        println!("Loaded plugin '{}' from {}", name, wasm_path_str);
    } else {
        anyhow::bail!("Failed to load plugin: {}", response.error);
    }
    Ok(())
}

pub async fn run_watch(path: Option<&Path>) -> Result<()> {
    // Keep local file watcher as before - watches local filesystem
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio::time::sleep;

    let tools_dir = match path {
        Some(p) => p.to_path_buf(),
        None => {
            let cfg = sisyphus_types::Config::default();
            PathBuf::from(&cfg.sandbox.wasm_tools_dir)
        }
    };

    println!("Watching for plugin changes in {}...", tools_dir.display());

    let mut last_mtime = std::fs::metadata(&tools_dir)?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .context("Invalid modification time")?;

    loop {
        sleep(Duration::from_secs(2)).await;
        if let Ok(metadata) = std::fs::metadata(&tools_dir) {
            if let Ok(mtime) = metadata.modified() {
                let current_mtime = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .context("Invalid modification time")?;
                if current_mtime > last_mtime {
                    println!("Detected changes in {}.", tools_dir.display());
                    last_mtime = current_mtime;
                }
            }
        }
    }
}

pub async fn run_unload(client: &mut BridgeClient, name: &str) -> Result<()> {
    let response = client.unload_plugin(name).await?;
    if response.success {
        println!("Unloaded plugin: {}", name);
    } else {
        anyhow::bail!("Failed to unload plugin: {}", response.error);
    }
    Ok(())
}
