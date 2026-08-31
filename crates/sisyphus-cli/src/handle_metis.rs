use std::sync::Arc;

use anyhow::Result;

use crate::commands::*;

/// Handle metis.
pub async fn handle_metis(
    sub: &MetisCommands,
    llm_client: Arc<dyn sisyphus_cli::tools::metis::LlmClient>,
) -> Result<()> {
    match sub {
        MetisCommands::Analyse { request, file } => {
            let text = match file {
                Some(f) => std::fs::read_to_string(f)?,
                None => request.join(" "),
            };
            if text.trim().is_empty() {
                anyhow::bail!("No request text provided. Pass inline text or use --file.");
            }
            let analysis = sisyphus_cli::tools::metis::analyse(&text, llm_client.as_ref())
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("=== Metis Analysis ===");
            println!("Summary: {}", analysis.summary);
            if !analysis.ambiguities.is_empty() {
                println!("\n--- Ambiguities ---");
                for a in &analysis.ambiguities {
                    println!("  [{}] {} — {}", a.severity, a.aspect, a.description);
                    if !a.options.is_empty() {
                        println!("    options: {}", a.options.join(" | "));
                    }
                    if let Some(r) = &a.recommended {
                        println!("    recommended: {r}");
                    }
                }
            }
            if !analysis.implicit_requirements.is_empty() {
                println!("\n--- Implicit Requirements ---");
                for i in &analysis.implicit_requirements {
                    println!(
                        "  [conf: {:.1}] {} — {}",
                        i.confidence, i.description, i.rationale
                    );
                }
            }
        },
    }
    Ok(())
}
