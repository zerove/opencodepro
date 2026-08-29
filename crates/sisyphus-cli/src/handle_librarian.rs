use std::sync::Arc;

use anyhow::Result;

use crate::commands::*;

/// Handle librarian.
pub async fn handle_librarian(
    sub: &LibrarianCommands,
    llm_client: Arc<dyn sisyphus_cli::tools::metis::LlmClient>,
) -> Result<()> {
    match sub {
        LibrarianCommands::Research { query, scope, num } => {
            let scope_enum: sisyphus_cli::tools::ResearchScope =
                scope.parse().map_err(|e: String| anyhow::anyhow!("{e}"))?;
            let report = sisyphus_cli::tools::research(query, scope_enum, *num).await?;
            println!("=== Research Report ===");
            println!("Query: {}", report.query);
            println!("Sources: {}", report.sources_consulted);
            println!("Confidence: {:.2}", report.confidence);
            println!("{}", report.synthesis);
            println!("\n--- Detailed Findings ---");
            for (i, f) in report.findings.iter().enumerate() {
                println!("[{i}] {} (relevance: {:.1})", f.claim, f.relevance);
                println!("    source: {}", f.source_url);
            }
        }
        LibrarianCommands::FetchDocs { url } => {
            let page = sisyphus_cli::tools::fetch_documentation(url).await?;
            println!("Title: {}", page.title);
            println!("Content length: {} bytes", page.content_length);
            println!("--- Preview ---");
            println!("{}", page.content_preview);
        }
        LibrarianCommands::Examples { query, language } => {
            let examples = sisyphus_cli::tools::find_examples(query, language).await?;
            if examples.is_empty() {
                println!("No examples found for '{}'.", query);
            } else {
                println!("Found {} example(s):", examples.len());
                for (i, ex) in examples.iter().enumerate() {
                    println!("[{i}] {} (from {})", ex.description, ex.source);
                    println!("    language: {}", ex.language);
                    if ex.code.len() > 200 {
                        println!("    code ({} chars): {}...", ex.code.len(), &ex.code[..200]);
                    } else {
                        println!("    code: {}", ex.code);
                    }
                }
            }
        }
        LibrarianCommands::Synthesise { report_file } => {
            let content = std::fs::read_to_string(report_file)?;
            let findings: Vec<sisyphus_cli::tools::ResearchFinding> =
                serde_json::from_str(&content)?;
            let synthesis = sisyphus_cli::tools::synthesise(&findings, llm_client.as_ref())
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("{}", synthesis);
        }
    }
    Ok(())
}
