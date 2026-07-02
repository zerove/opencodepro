use anyhow::Result;

use crate::commands::ProviderCommands;

pub(crate) fn handle_provider(sub: &ProviderCommands, config: &sisyphus_types::Config) -> Result<()> {
    match sub {
        ProviderCommands::List => {
            let providers = &config.llm.providers;
            if providers.is_empty() {
                println!("No providers configured.");
                println!(
                    "  Using model '{}' at {}",
                    config.llm.model, config.llm.endpoint,
                );
            } else {
                println!("Registered providers (fallback order):");
                for (i, p) in providers.iter().enumerate() {
                    let key_status = if p.api_key.is_empty() || p.api_key.starts_with("env:") {
                        "key: <env>"
                    } else {
                        "key: set"
                    };
                    println!(
                        "  {}. {} (kind: {}, model: {}, {})",
                        i + 1,
                        p.name,
                        p.kind,
                        p.model,
                        key_status,
                    );
                }
            }
        }
        ProviderCommands::Test { name, prompt } => {
            let providers = &config.llm.providers;
            let provider = providers.iter().find(|p| p.name == *name);
            match provider {
                Some(p) => {
                    println!(
                        "Testing provider '{}' (kind: {}, model: {})...",
                        p.name, p.kind, p.model
                    );
                    let api_key = p.resolve_api_key();
                    if api_key.is_empty() {
                        eprintln!(
                            "API key for '{}' is empty. Check env var or config.",
                            p.name
                        );
                    } else {
                        println!(
                            "API key resolved ({} chars), endpoint: {}",
                            api_key.len(),
                            p.base_url.as_deref().unwrap_or("(default)"),
                        );
                        println!("Prompt: {}", prompt);
                        println!("Test call would go here (API call disabled in CLI test mode).");
                    }
                }
                None => {
                    eprintln!(
                        "Provider '{}' not found. Use `provider list` to see available providers.",
                        name
                    );
                }
            }
        }
        ProviderCommands::Fallback => {
            let providers = &config.llm.providers;
            println!("Fallback chain for provider routing:");
            println!(
                "  1. Try registered providers in order ({} total)",
                providers.len()
            );
            for (i, p) in providers.iter().enumerate() {
                println!("     {}. {} (kind: {}, model: {})", i + 1, p.name, p.kind, p.model);
            }
            println!(
                "  2. Fallback to default endpoint: {} with model '{}'",
                config.llm.endpoint, config.llm.model
            );
            println!();
            println!("Error handling in fallback:");
            println!("  - Transient/RateLimited errors -> skip to next provider");
            println!("  - Permanent/Auth errors -> abort immediately");
            println!("  - All providers exhausted -> fallback to default endpoint");
        }
    }
    Ok(())
}
