use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::commands::ToolCommands;
use crate::Cli;

pub(crate) fn handle_tool(
    sub: &ToolCommands,
    cli: &Cli,
    sisyphus_cfg: &sisyphus_types::Config,
) -> Result<()> {
    match sub {
        ToolCommands::RegisterMcp { prefix, command } => {
            let command_str = command.join(" ");
            let config_path = cli.config.as_deref().map(|p| p.to_path_buf()).unwrap_or_else(|| {
                [
                    "opencode.json",
                    "opencode.jsonc",
                    ".opencode.json",
                    "sisyphus.toml",
                ]
                .iter()
                .find(|p| Path::new(p).exists())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("sisyphus.toml"))
            });

            let mut cfg = if config_path.exists() {
                let path_str = config_path.to_string_lossy();
                match sisyphus_types::Config::from_file(&path_str) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("Warning: failed to load config: {e}, using defaults");
                        sisyphus_types::Config::default()
                    },
                }
            } else {
                sisyphus_types::Config::default()
            };

            cfg.mcp.stdio_servers.insert(prefix.clone(), command_str.clone());

            let path_str = config_path.to_string_lossy().to_string();
            cfg.save_to_file(&path_str)?;

            println!("Registered MCP tool '{}' -> {}", prefix, command_str);
            println!("Config saved to: {}", config_path.display());
            println!("Restart the bridge for changes to take effect.");
        },
        ToolCommands::List => {
            let providers = &sisyphus_cfg.mcp.stdio_servers;
            let http_servers = &sisyphus_cfg.mcp.http_servers;
            println!("Registered MCP tools:");
            if providers.is_empty() && http_servers.is_empty() {
                println!("  (none)");
                println!("Use `tool register-mcp <prefix> <command>` to add one.");
            }
            if !providers.is_empty() {
                println!("\n  Stdio servers:");
                for (prefix, cmd) in providers {
                    println!("    {prefix}: {cmd}");
                }
            }
            if !http_servers.is_empty() {
                println!("\n  HTTP servers:");
                for (prefix, url) in http_servers {
                    println!("    {prefix}: {url}");
                }
            }
        },
    }
    Ok(())
}
