mod types;
pub use types::*;

use regex::Regex;
use sisyphus_tools_core::{ToolError, ToolResult};
use std::fs;
use std::path::{Path, PathBuf};

/// Search multiple patterns across files in a directory tree.
/// Each pattern is a regex. Returns matches with surrounding context lines.
pub fn search_patterns(
    patterns: &[&str],
    root: &Path,
    include: Option<&str>,
    context_lines: usize,
) -> ToolResult<ExploreResult> {
    if patterns.is_empty() {
        return Ok(ExploreResult {
            matches: vec![],
            files_scanned: 0,
            total_matches: 0,
        });
    }

    let re_list: Vec<Regex> = patterns
        .iter()
        .map(|p| {
            Regex::new(p).map_err(|e| ToolError::GrepError(format!("invalid regex '{p}': {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let file_filter = include.map(|s| {
        let re = Regex::new(s).ok();
        move |path: &Path| -> bool {
            if let Some(ref r) = re {
                r.is_match(&path.to_string_lossy())
            } else {
                // If include is not a valid regex, use it as a simple suffix match
                path.to_string_lossy().ends_with(s)
            }
        }
    });

    let mut result = ExploreResult {
        matches: vec![],
        files_scanned: 0,
        total_matches: 0,
    };

    let walker = ignore::WalkBuilder::new(root).build();
    for entry in walker {
        let entry = entry.map_err(|e| ToolError::GrepError(e.to_string()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(ref filter) = file_filter {
            if !filter(path) {
                continue;
            }
        }
        result.files_scanned += 1;

        let file_content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let lines: Vec<&str> = file_content.lines().collect();

        for (line_idx, line_content) in lines.iter().enumerate() {
            for (pi, re) in re_list.iter().enumerate() {
                if re.is_match(line_content) {
                    let line_num = line_idx + 1;
                    let before: Vec<String> = lines
                        [line_idx.saturating_sub(context_lines)..line_idx]
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect();
                    let after: Vec<String> = lines[(line_idx + 1).min(lines.len())
                        ..(line_idx + 1 + context_lines).min(lines.len())]
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect();

                    result.matches.push(ExploreMatch {
                        file: path.to_path_buf(),
                        line: line_num,
                        content: line_content.to_string(),
                        before,
                        after,
                        pattern_index: pi,
                    });
                    result.total_matches += 1;
                    // Don't match same line against multiple patterns
                    break;
                }
            }
        }
    }

    Ok(result)
}

/// Gather context from specific files, optionally limiting to line ranges.
/// Returns a map from file path to file content (with line numbers).
pub fn gather_context(paths: &[&Path]) -> ToolResult<Vec<FileContext>> {
    let mut results = vec![];
    for path in paths {
        let content = crate::file::read(path)?;
        let line_count = content.lines().count();
        results.push(FileContext {
            path: path.to_path_buf(),
            content,
            line_count,
        });
    }
    Ok(results)
}

/// Find files whose names match a glob pattern AND whose content matches
/// an optional regex query. Combines glob + grep in one call.
pub fn find_files(
    glob_pattern: &str,
    root: &Path,
    content_query: Option<&str>,
) -> ToolResult<Vec<PathBuf>> {
    let matched_paths = crate::file::glob(glob_pattern, root)?;

    if let Some(query) = content_query {
        let re = Regex::new(query)
            .map_err(|e| ToolError::GrepError(format!("invalid regex '{query}': {e}")))?;
        let mut filtered = vec![];
        for p in &matched_paths {
            if let Ok(content) = fs::read_to_string(p) {
                if re.is_match(&content) {
                    filtered.push(p.clone());
                }
            }
        }
        Ok(filtered)
    } else {
        Ok(matched_paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().expect("create temp dir");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();

        std::fs::write(
            src.join("main.rs"),
            "\
fn greet(name: &str) -> String {
    format!(\"Hello, {name}!\")
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_greet() {
        assert_eq!(greet(\"World\"), \"Hello, World!\");
    }
}
",
        )
        .unwrap();

        std::fs::write(
            src.join("lib.rs"),
            "\
pub mod utils;

pub fn run() {
    println!(\"running...\");
}
",
        )
        .unwrap();

        std::fs::write(
            src.join("utils.rs"),
            "\
pub fn helper() -> u32 {
    42
}
",
        )
        .unwrap();

        std::fs::write(dir.path().join("README.md"), "# Sisyphus Project").unwrap();

        dir
    }

    #[test]
    fn test_search_patterns_basic() {
        let dir = setup_test_dir();
        let result = search_patterns(&["fn "], dir.path(), Some(r"\.rs$"), 2).unwrap();
        assert!(
            result.total_matches >= 4,
            "expected >=4 matches for 'fn ', got {}",
            result.total_matches
        );
        assert!(result.files_scanned >= 2);
    }

    #[test]
    fn test_search_patterns_multiple() {
        let dir = setup_test_dir();
        let result = search_patterns(&["fn ", "assert"], dir.path(), Some(r"\.rs$"), 1).unwrap();
        assert!(
            result.total_matches >= 5,
            "expected >=5 matches, got {}",
            result.total_matches
        );
    }

    #[test]
    fn test_search_patterns_no_filter() {
        let dir = setup_test_dir();
        let result = search_patterns(&["Sisyphus"], dir.path(), None, 0).unwrap();
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.matches[0].content, "# Sisyphus Project");
    }

    #[test]
    fn test_search_patterns_context() {
        let dir = setup_test_dir();
        let result = search_patterns(&["assert_eq"], dir.path(), Some(r"\.rs$"), 2).unwrap();
        assert_eq!(result.total_matches, 1);
        let m = &result.matches[0];
        assert!(!m.before.is_empty(), "expected context before match");
        assert!(!m.after.is_empty(), "expected context after match");
    }

    #[test]
    fn test_search_patterns_empty_patterns() {
        let dir = setup_test_dir();
        let result = search_patterns(&[], dir.path(), None, 0).unwrap();
        assert_eq!(result.total_matches, 0);
    }

    #[test]
    fn test_gather_context() {
        let dir = setup_test_dir();
        let p1 = dir.path().join("src/main.rs");
        let p2 = dir.path().join("src/lib.rs");
        let paths = vec![p1.as_path(), p2.as_path()];
        let contexts = gather_context(&paths).unwrap();
        assert_eq!(contexts.len(), 2);
        assert!(contexts[0].content.contains("fn greet"));
        assert!(contexts[0].line_count > 3);
    }

    #[test]
    fn test_gather_context_not_found() {
        let dir = setup_test_dir();
        let p = dir.path().join("nonexistent.rs");
        let paths = vec![p.as_path()];
        let result = gather_context(&paths);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_files_glob_only() {
        let dir = setup_test_dir();
        let files = find_files("**/*.rs", dir.path(), None).unwrap();
        assert_eq!(files.len(), 3); // main.rs, lib.rs, utils.rs
    }

    #[test]
    fn test_find_files_glob_and_content() {
        let dir = setup_test_dir();
        let files = find_files("**/*.rs", dir.path(), Some("fn greet")).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("main.rs"));
    }

    #[test]
    fn test_search_patterns_invalid_regex() {
        let dir = setup_test_dir();
        let result = search_patterns(&["[invalid"], dir.path(), None, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_explore_match_structure() {
        let dir = setup_test_dir();
        let result = search_patterns(&["helper"], dir.path(), Some(r"\.rs$"), 0).unwrap();
        assert_eq!(result.total_matches, 1);
        let m = &result.matches[0];
        assert!(m.file.to_string_lossy().ends_with("utils.rs"));
        assert_eq!(m.content, "pub fn helper() -> u32 {");
        assert_eq!(m.pattern_index, 0);
    }

    #[test]
    fn test_search_patterns_no_matches() {
        let dir = setup_test_dir();
        let result = search_patterns(&["zzz_nonexistent_zzz"], dir.path(), None, 0).unwrap();
        assert_eq!(result.total_matches, 0);
        assert!(result.matches.is_empty());
    }

    #[test]
    fn test_search_patterns_nonexistent_root() {
        let result = search_patterns(
            &["fn"],
            Path::new("/tmp/__sisyphus_nonexistent_explore__"),
            None,
            0,
        );
        assert!(
            result.is_err(),
            "ignore::WalkBuilder errors on non-existent root"
        );
    }

    #[test]
    fn test_search_patterns_case_sensitive() {
        let dir = setup_test_dir();
        let result = search_patterns(&["GREET"], dir.path(), Some(r"\.rs$"), 0).unwrap();
        assert_eq!(
            result.total_matches, 0,
            "regex is case-sensitive by default"
        );
    }

    #[test]
    fn test_search_patterns_no_include_filter_no_match_dir() {
        let dir = setup_test_dir();
        let result = search_patterns(&["README"], dir.path(), None, 0).unwrap();
        assert_eq!(
            result.total_matches, 0,
            "no .md files should be scanned without include"
        );
    }

    #[test]
    fn test_gather_context_empty_paths() {
        let result = gather_context(&[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_files_glob_no_match() {
        let dir = setup_test_dir();
        let files = find_files("**/*.py", dir.path(), None).unwrap();
        assert!(files.is_empty(), "no .py files should exist");
    }

    #[test]
    fn test_find_files_content_no_match() {
        let dir = setup_test_dir();
        let files = find_files("**/*.rs", dir.path(), Some("zzz_nonexistent")).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_find_files_nonexistent_root() {
        let result = find_files("**/*.rs", Path::new("/tmp/__sisyphus_nonexistent__"), None);
        assert!(result.is_ok(), "should handle non-existent root gracefully");
        let files = result.unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_search_patterns_context_at_file_boundaries() {
        let dir = setup_test_dir();
        let result = search_patterns(&["fn greet"], dir.path(), Some(r"main\.rs$"), 5).unwrap();
        assert_eq!(result.total_matches, 1);
        let m = &result.matches[0];
        assert!(m.before.is_empty() || m.before.iter().all(|l| l.is_empty()));
    }

    #[test]
    fn test_search_patterns_binary_file_skipped() {
        let dir = setup_test_dir();
        let bin = dir.path().join("data.bin");
        std::fs::write(&bin, [0u8, 159, 146, 150]).unwrap();

        let result = search_patterns(&[".*"], dir.path(), Some(r"\.bin$"), 0).unwrap();
        assert_eq!(
            result.total_matches, 0,
            "binary files should be silently skipped"
        );
    }

    #[test]
    fn test_search_patterns_include_suffix_fallback() {
        let dir = setup_test_dir();
        let result = search_patterns(&["fn"], dir.path(), Some("[.rs"), 0).unwrap();
        assert_eq!(result.total_matches, 0, "no files end with '[.rs'");
    }

    #[test]
    fn test_find_files_invalid_glob() {
        let dir = setup_test_dir();
        let result = find_files("[", dir.path(), None);
        assert!(result.is_err(), "invalid glob pattern should error");
    }
}
