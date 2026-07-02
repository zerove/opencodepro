use std::path::Path;

use anyhow::Result;

use crate::commands::*;

pub fn handle_oracle(sub: &OracleCommands) -> Result<()> {
    match sub {
        OracleCommands::AnalyzeFile { path } => {
            let analysis = sisyphus_tools::analyze_file(path)?;
            println!("File: {}", path.display());
            println!("  Lines: {}", analysis.line_count);
            println!("  Imports:");
            for imp in &analysis.imports {
                println!("    {imp}");
            }
            println!("  Functions:");
            for func in &analysis.functions {
                println!("    {} at line {}", func.name, func.line);
            }
        }
        OracleCommands::AnalyzeDir { path, include } => {
            let summary = sisyphus_tools::analyze_directory(path, include.as_deref())?;
            println!("Directory: {}", path.display());
            println!("  Files analyzed: {}", summary.file_count);
            println!("  Total lines: {}", summary.total_lines);
        }
        OracleCommands::CodeReview { paths } => {
            let path_refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
            let issues = sisyphus_tools::code_review(&path_refs)?;
            if issues.is_empty() {
                println!("No issues found.");
            } else {
                for issue in &issues {
                    println!(
                        "  [{}] {} (line {}) — {}",
                        issue.severity,
                        issue.file.display(),
                        issue.line,
                        issue.message
                    );
                }
            }
        }
    }
    Ok(())
}
