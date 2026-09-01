//! Integration tests for sisyphus-tools-system `explore` module.
//!
//! Exercises the public `search_patterns` and `gather_context` functions
//! which scan a directory tree for regex matches with surrounding context
//! lines. These are the building blocks used by the "explore" tool to
//! quickly locate symbols in a codebase.

use sisyphus_tools_system::explore::{gather_context, search_patterns, ExploreMatch, FileContext};
use std::fs;
use tempfile::TempDir;

fn write(dir: &std::path::Path, rel: &str, body: &str) -> std::path::PathBuf {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(&p, body).expect("write");
    p
}

// ---------------------------------------------------------------------------
// search_patterns
// ---------------------------------------------------------------------------

#[test]
fn search_empty_patterns_returns_empty_result() {
    let dir = TempDir::new().expect("tempdir");
    write(dir.path(), "a.rs", "fn main() {}\n");
    let r = search_patterns(&[], dir.path(), None, 0).expect("ok");
    assert_eq!(r.matches.len(), 0);
    assert_eq!(r.files_scanned, 0);
    assert_eq!(r.total_matches, 0);
}

#[test]
fn search_finds_matching_files() {
    let dir = TempDir::new().expect("tempdir");
    write(dir.path(), "a.rs", "fn main() {}\n");
    write(dir.path(), "b.rs", "fn other() {}\n");
    write(dir.path(), "c.txt", "not rust\n");
    let r = search_patterns(&["fn main"], dir.path(), None, 0).expect("ok");
    assert_eq!(r.matches.len(), 1);
    assert!(r.matches[0].file.ends_with("a.rs"));
    assert_eq!(r.matches[0].line, 1);
    assert!(r.matches[0].content.contains("fn main"));
}

#[test]
fn search_total_matches_aggregates_across_files() {
    let dir = TempDir::new().expect("tempdir");
    write(dir.path(), "a.rs", "foo\nfoo\nfoo\n");
    write(dir.path(), "b.rs", "foo\n");
    let r = search_patterns(&["foo"], dir.path(), None, 0).expect("ok");
    assert_eq!(r.total_matches, 4);
    assert_eq!(r.matches.len(), 4);
}

#[test]
fn search_invalid_regex_returns_error() {
    let dir = TempDir::new().expect("tempdir");
    let r = search_patterns(&["(unclosed"], dir.path(), None, 0);
    assert!(r.is_err());
}

#[test]
fn search_with_include_filter_only_scans_matching_files() {
    let dir = TempDir::new().expect("tempdir");
    write(dir.path(), "a.rs", "fn main() {}\n");
    write(dir.path(), "b.txt", "fn main() {}\n");
    // include is matched against the full path; use a simple suffix
    // string that is not a regex (the implementation falls back to
    // `ends_with` for non-regex input).
    let r = search_patterns(&["fn main"], dir.path(), Some(".rs"), 0).expect("ok");
    assert_eq!(r.matches.len(), 1);
    assert!(
        r.matches[0].file.to_string_lossy().ends_with(".rs"),
        "got {:?}",
        r.matches[0].file
    );
}

#[test]
fn search_with_context_lines_includes_before_and_after() {
    let dir = TempDir::new().expect("tempdir");
    write(
        dir.path(),
        "a.rs",
        "line1\nline2\nline3 TARGET\nline4\nline5\n",
    );
    let r = search_patterns(&["TARGET"], dir.path(), None, 2).expect("ok");
    assert_eq!(r.matches.len(), 1);
    let m = &r.matches[0];
    assert_eq!(m.before.len(), 2);
    assert_eq!(m.before[0], "line1");
    assert_eq!(m.before[1], "line2");
    assert_eq!(m.after.len(), 2);
    assert_eq!(m.after[0], "line4");
    assert_eq!(m.after[1], "line5");
}

#[test]
fn search_at_file_start_has_no_before_context() {
    let dir = TempDir::new().expect("tempdir");
    write(dir.path(), "a.rs", "TARGET first line\nsecond line\n");
    let r = search_patterns(&["TARGET"], dir.path(), None, 3).expect("ok");
    let m = &r.matches[0];
    assert!(m.before.is_empty(), "no lines before line 1");
    assert_eq!(m.after.len(), 1);
    assert_eq!(m.after[0], "second line");
}

#[test]
fn search_at_file_end_has_no_after_context() {
    let dir = TempDir::new().expect("tempdir");
    write(dir.path(), "a.rs", "first\nsecond\nlast TARGET\n");
    let r = search_patterns(&["TARGET"], dir.path(), None, 3).expect("ok");
    let m = &r.matches[0];
    assert_eq!(m.before.len(), 2);
    assert!(m.after.is_empty());
}

#[test]
fn search_pattern_index_distinguishes_multiple_patterns() {
    let dir = TempDir::new().expect("tempdir");
    write(dir.path(), "a.rs", "alpha\nbeta\ngamma\n");
    let r = search_patterns(&["alpha", "beta", "gamma"], dir.path(), None, 0).expect("ok");
    assert_eq!(r.matches.len(), 3);
    let mut indices: Vec<usize> = r.matches.iter().map(|m| m.pattern_index).collect();
    indices.sort();
    assert_eq!(indices, vec![0, 1, 2]);
}

#[test]
fn search_recurses_into_subdirectories() {
    let dir = TempDir::new().expect("tempdir");
    write(dir.path(), "a.rs", "match_here\n");
    write(dir.path(), "sub/b.rs", "match_here\n");
    write(dir.path(), "sub/deep/c.rs", "match_here\n");
    let r = search_patterns(&["match_here"], dir.path(), None, 0).expect("ok");
    assert_eq!(r.matches.len(), 3);
    // The walker should have scanned 3 files.
    assert!(r.files_scanned >= 3);
}

// ---------------------------------------------------------------------------
// gather_context
// ---------------------------------------------------------------------------

#[test]
fn gather_context_returns_content_for_existing_files() {
    let dir = TempDir::new().expect("tempdir");
    write(dir.path(), "a.rs", "line1\nline2\nline3\n");
    let files = [dir.path().join("a.rs")];
    let ctx = gather_context(&files.iter().map(|p| p.as_path()).collect::<Vec<_>>()).expect("ok");
    assert_eq!(ctx.len(), 1);
    let fc = &ctx[0];
    assert!(fc.path.ends_with("a.rs"));
    assert!(fc.content.contains("line2"));
    assert_eq!(fc.line_count, 3);
}

#[test]
fn gather_context_returns_error_for_missing_files() {
    let dir = TempDir::new().expect("tempdir");
    let missing = dir.path().join("nope.rs");
    let paths = vec![missing.as_path()];
    let result = gather_context(&paths);
    // Missing files propagate as an error (read failure).
    assert!(result.is_err());
}

#[test]
fn gather_context_empty_input_returns_empty() {
    let ctx = gather_context(&[]).expect("ok");
    assert!(ctx.is_empty());
}

// ---------------------------------------------------------------------------
// Type construction / Debug
// ---------------------------------------------------------------------------

#[test]
fn explore_match_and_file_context_debug() {
    let m = ExploreMatch {
        file: std::path::PathBuf::from("/x.rs"),
        line: 5,
        content: "x".into(),
        before: vec!["a".into()],
        after: vec!["b".into()],
        pattern_index: 0,
    };
    let s = format!("{m:?}");
    assert!(s.contains("x.rs"));
    assert!(s.contains("line: 5"));

    let fc = FileContext {
        path: std::path::PathBuf::from("/y.rs"),
        content: "y".into(),
        line_count: 1,
    };
    let s = format!("{fc:?}");
    assert!(s.contains("y.rs"));
}
