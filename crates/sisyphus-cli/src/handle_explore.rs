use std::path::Path;

use anyhow::Result;

use crate::commands::*;

pub fn handle_explore(sub: &ExploreCommands) -> Result<()> {
    match sub {
        ExploreCommands::Search {
            patterns,
            paths,
            max_matches: _,
            context_lines,
            include,
        } => {
            let root = paths
                .first()
                .map(|p| p.as_path())
                .unwrap_or_else(|| Path::new("."));
            let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
            let results = sisyphus_tools::search_patterns(
                &pattern_refs,
                root,
                include.as_deref(),
                *context_lines,
            )?;
            for m in &results.matches {
                println!("{}:{} — {}", m.file.display(), m.line, m.content.trim());
            }
            if results.matches.is_empty() {
                println!("No matches found.");
            }
        }
        ExploreCommands::FindFiles {
            pattern,
            root,
            limit: _,
        } => {
            let files = sisyphus_tools::find_files(pattern, root, None)?;
            for f in &files {
                println!("{}", f.display());
            }
            if files.is_empty() {
                println!("No files matched.");
            }
        }
    }
    Ok(())
}
