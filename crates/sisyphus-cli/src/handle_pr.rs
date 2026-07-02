use std::sync::Arc;

use anyhow::Result;

use crate::commands::PrCommands;

pub(crate) async fn handle_pr(
    sub: &PrCommands,
    llm_client: &Arc<dyn sisyphus_cli::tools::metis::LlmClient>,
) -> Result<()> {
    match sub {
        PrCommands::List => {
            let prs = sisyphus_cli::tools::PrManager::list_prs()?;
            if prs.is_empty() {
                println!("No open pull requests found.");
            } else {
                println!("{:<6} {:<40} {:<15} {:<10}", "Num", "Title", "Author", "State");
                println!("{}", "-".repeat(71));
                for pr in prs {
                    println!(
                        "{:<6} {:<40} {:<15} {:<10}",
                        pr.number,
                        pr.title.chars().take(37).collect::<String>()
                            + if pr.title.len() > 37 { "..." } else { "" },
                        pr.author,
                        pr.state
                    );
                }
            }
        }
        PrCommands::Checkout { number } => {
            sisyphus_cli::tools::PrManager::checkout_pr(*number)?;
            println!("Checked out PR #{}", number);
        }
        PrCommands::Review { number, post, format } => {
            let ci_cfg = sisyphus_cli::tools::CiConfig::from_env();
            let use_json = format == "json" || ci_cfg.output_format == "json";

            if use_json {
                println!(
                    "{{\"status\":\"running\",\"pr_number\":{},\"message\":\"Running LLM review...\"}}",
                    number
                );
            } else {
                println!("Running LLM review for PR #{}...", number);
            }

            let review =
                sisyphus_cli::tools::PrManager::review_pr(*number, llm_client.clone()).await?;

            if use_json {
                let escaped_review = review
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r");
                println!(
                    "{{\"status\":\"complete\",\"pr_number\":{},\"review\":\"{}\"}}",
                    number, escaped_review
                );
            } else {
                println!("\n=== PR Review Result ===\n");
                println!("{}", review);
            }

            if *post || ci_cfg.ci_mode {
                match sisyphus_cli::tools::post_pr_comment(*number, &review) {
                    Ok(url) => {
                        if use_json {
                            println!(
                                "{{\"status\":\"comment_posted\",\"pr_number\":{},\"url\":\"{}\"}}",
                                number, url
                            );
                        } else {
                            println!("\nReview comment posted: {}", url);
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to post PR comment: {}", e);
                        if ci_cfg.ci_mode {
                            eprintln!("(non-fatal in CI mode)");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
