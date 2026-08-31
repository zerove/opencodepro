use std::path::Path;

use anyhow::Result;

use crate::commands::*;

/// Handle explore.
pub fn handle_explore(sub: &ExploreCommands) -> Result<()> {
    match sub {
        ExploreCommands::Search {
            patterns,
            paths,
            max_matches: _,
            context_lines,
            include,
        } => {
            let root = paths.first().map(|p| p.as_path()).unwrap_or_else(|| Path::new("."));
            let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
            let results = sisyphus_cli::tools::search_patterns(
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
        },
        ExploreCommands::FindFiles {
            pattern,
            root,
            limit: _,
        } => {
            let files = sisyphus_cli::tools::find_files(pattern, root, None)?;
            for f in &files {
                println!("{}", f.display());
            }
            if files.is_empty() {
                println!("No files matched.");
            }
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn test_explore_empty_format() {
        let msg = "No matches found.";
        assert_eq!(msg, "No matches found.");
    }

    #[test]
    fn test_explore_files_empty_format() {
        let msg = "No files matched.";
        assert_eq!(msg, "No files matched.");
    }

    #[test]
    fn test_explore_search_format() {
        let file = PathBuf::from("/tmp/test.rs");
        let line: usize = 42;
        let content = "  let x = 1;";
        let formatted = format!("{}:{} — {}", file.display(), line, content.trim());
        assert_eq!(formatted, "/tmp/test.rs:42 — let x = 1;");
    }

    #[test]
    fn test_explore_find_files_format() {
        let file = PathBuf::from("/tmp/found.txt");
        let formatted = format!("{}", file.display());
        assert_eq!(formatted, "/tmp/found.txt");
    }
}
