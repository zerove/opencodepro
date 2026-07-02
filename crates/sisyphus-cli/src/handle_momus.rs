use std::sync::Arc;

use anyhow::Result;

use crate::commands::MomusCommands;

pub(crate) async fn handle_momus(
    sub: &MomusCommands,
    llm_client: &Arc<dyn sisyphus_tools::metis::LlmClient>,
) -> Result<()> {
    match sub {
        MomusCommands::ReviewPlan { plan_file } => {
            let content = std::fs::read_to_string(plan_file)?;
            let name = plan_file
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed".to_string());
            let review =
                sisyphus_tools::review_plan(llm_client.as_ref(), &content, &name).await;
            println!("{}", review.summary);
            if !review.comments.is_empty() {
                println!("\n--- Issues ---");
                for c in &review.comments {
                    println!("  [{}] {} — {}", c.severity, c.category, c.message);
                    if let Some(s) = &c.suggestion {
                        println!("       suggestion: {s}");
                    }
                }
            }
        }
        MomusCommands::ReviewCode { path } => {
            let content = std::fs::read_to_string(path)?;
            let review = sisyphus_tools::review_code(llm_client.as_ref(), path, &content)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("{}", review.summary);
            if !review.comments.is_empty() {
                println!("\n--- Issues ---");
                for c in &review.comments {
                    if let Some(line) = c.line {
                        println!(
                            "  [{}] {}:{} — {} ({})",
                            c.severity,
                            c.file.as_deref().unwrap_or("?"),
                            line,
                            c.message,
                            c.category
                        );
                    } else {
                        println!("  [{}] {} — {}", c.severity, c.message, c.category);
                    }
                    if let Some(s) = &c.suggestion {
                        println!("       suggestion: {s}");
                    }
                }
            }
        }
        MomusCommands::ReviewArch { text } => {
            let review = sisyphus_tools::review_architecture(llm_client.as_ref(), text).await;
            println!("{}", review.summary);
            if !review.comments.is_empty() {
                println!("\n--- Issues ---");
                for c in &review.comments {
                    println!("  [{}] {} — {}", c.severity, c.category, c.message);
                    if let Some(s) = &c.suggestion {
                        println!("       suggestion: {s}");
                    }
                }
            }
        }
        MomusCommands::ReviewDir { path } => {
            let walker = std::fs::read_dir(path)?;
            let mut reviews = Vec::new();
            for entry in walker {
                let entry = entry?;
                let file_path = entry.path();
                if file_path.extension().map(|e| e == "rs").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&file_path) {
                        if let Ok(review) = sisyphus_tools::review_code(
                            llm_client.as_ref(),
                            &file_path,
                            &content,
                        )
                        .await
                        {
                            reviews.push(review);
                        }
                    }
                }
            }
            let total_passed = reviews.iter().filter(|r| r.passed).count();
            let total_failed = reviews.iter().filter(|r| !r.passed).count();
            println!(
                "Reviewed {} files: {} passed, {} failed",
                reviews.len(),
                total_passed,
                total_failed
            );
            for r in &reviews {
                println!("  {} — {}", r.target, if r.passed { "✅" } else { "❌" });
                for c in &r.comments {
                    if let Some(line) = c.line {
                        println!("    [{}] line {}: {}", c.severity, line, c.message);
                    } else {
                        println!("    [{}] {}", c.severity, c.message);
                    }
                }
            }
        }
    }
    Ok(())
}
