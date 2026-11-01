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
            println!("\nAvailable self-programming presets (use `tool bootstrap <id>`):");
            for id in sisyphus_types::tools::PresetToolTemplates::ALL_IDS {
                println!("    {id}");
            }
        },
        ToolCommands::Bootstrap { template_id } => {
            match sisyphus_types::tools::PresetToolTemplates::get_preset(template_id) {
                Some(spec) => {
                    println!("✅ Resolved preset template: {}", spec.tool_id);
                    println!("   Capability : {}", spec.required_capability);
                    println!("   Success    : {}", spec.success_criteria);
                    println!(
                        "   Input      : {}",
                        serde_json::to_string(&spec.expected_input).unwrap_or_default()
                    );
                    println!(
                        "   Output     : {}",
                        serde_json::to_string(&spec.expected_output).unwrap_or_default()
                    );
                    println!("\nNext: run a goal that needs this capability; the self-programming");
                    println!("loop will compile and register it automatically.");
                },
                None => {
                    let available = sisyphus_types::tools::PresetToolTemplates::ALL_IDS.join(", ");
                    anyhow::bail!(
                        "Unknown preset template: {template_id} (available: {available})"
                    );
                },
            }
        },
    }
    Ok(())
}
