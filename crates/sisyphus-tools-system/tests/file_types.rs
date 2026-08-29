//! Integration tests for sisyphus-tools-system `file` module's
//! pure data types: `GrepMatch`, `ExploreMatch`, `ExploreResult`, `FileContext`.

use sisyphus_tools_system::file::{ExploreMatch, ExploreResult, FileContext, GrepMatch};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// GrepMatch
// ---------------------------------------------------------------------------

#[test]
fn grep_match_field_access() {
    let m = GrepMatch {
        file: PathBuf::from("/src/main.rs"),
        line: 42,
        content: "fn main() {}".into(),
    };
    assert_eq!(m.file, PathBuf::from("/src/main.rs"));
    assert_eq!(m.line, 42);
    assert_eq!(m.content, "fn main() {}");
}

#[test]
fn grep_match_empty_content() {
    let m = GrepMatch {
        file: PathBuf::from("/a"),
        line: 1,
        content: String::new(),
    };
    assert_eq!(m.line, 1);
    assert!(m.content.is_empty());
}

#[test]
fn grep_match_debug() {
    let m = GrepMatch {
        file: PathBuf::from("/a"),
        line: 5,
        content: "x".into(),
    };
    let s = format!("{m:?}");
    assert!(s.contains("a"));
    assert!(s.contains("5"));
    assert!(s.contains("x"));
}

// ---------------------------------------------------------------------------
// ExploreMatch
// ---------------------------------------------------------------------------

#[test]
fn explore_match_field_access() {
    let m = ExploreMatch {
        file: PathBuf::from("/src/lib.rs"),
        line: 10,
        content: "pub fn x()".into(),
        before: vec!["use std::io;".into()],
        after: vec!["}".into(), "".into()],
        pattern_index: 0,
    };
    assert_eq!(m.file, PathBuf::from("/src/lib.rs"));
    assert_eq!(m.line, 10);
    assert_eq!(m.content, "pub fn x()");
    assert_eq!(m.before.len(), 1);
    assert_eq!(m.after.len(), 2);
}

#[test]
fn explore_match_no_context() {
    let m = ExploreMatch {
        file: PathBuf::from("/a"),
        line: 1,
        content: "x".into(),
        before: vec![],
        after: vec![],
        pattern_index: 1,
    };
    assert!(m.before.is_empty());
    assert!(m.after.is_empty());
}

// ---------------------------------------------------------------------------
// ExploreResult
// ---------------------------------------------------------------------------

#[test]
fn explore_result_field_access() {
    let r = ExploreResult {
        matches: vec![ExploreMatch {
            file: PathBuf::from("/a"),
            line: 1,
            content: "x".into(),
            before: vec![],
            after: vec![],
            pattern_index: 0,
        }],
        files_scanned: 10,
        total_matches: 5,
    };
    assert_eq!(r.matches.len(), 1);
    assert_eq!(r.files_scanned, 10);
    assert_eq!(r.total_matches, 5);
}

#[test]
fn explore_result_empty() {
    let r = ExploreResult {
        matches: vec![],
        files_scanned: 0,
        total_matches: 0,
    };
    assert!(r.matches.is_empty());
    assert_eq!(r.files_scanned, 0);
}

#[test]
fn explore_result_total_matches_larger_than_vec() {
    // total_matches is a counter, can exceed matches.len() if paginated
    let r = ExploreResult {
        matches: vec![ExploreMatch {
            file: PathBuf::from("/a"),
            line: 1,
            content: "x".into(),
            before: vec![],
            after: vec![],
            pattern_index: 2,
        }],
        files_scanned: 1,
        total_matches: 100,
    };
    assert_eq!(r.total_matches, 100);
    assert_eq!(r.matches.len(), 1);
}

// ---------------------------------------------------------------------------
// FileContext
// ---------------------------------------------------------------------------

#[test]
fn file_context_field_access() {
    let c = FileContext {
        path: PathBuf::from("/a.txt"),
        content: "hello\nworld".into(),
        line_count: 2,
    };
    assert_eq!(c.path, PathBuf::from("/a.txt"));
    assert_eq!(c.content, "hello\nworld");
    assert_eq!(c.line_count, 2);
}

#[test]
fn file_context_empty_file() {
    let c = FileContext {
        path: PathBuf::from("/empty"),
        content: String::new(),
        line_count: 0,
    };
    assert!(c.content.is_empty());
    assert_eq!(c.line_count, 0);
}

#[test]
fn file_context_clone() {
    let a = FileContext {
        path: PathBuf::from("/x"),
        content: "y".into(),
        line_count: 1,
    };
    let b = a.clone();
    assert_eq!(a.path, b.path);
    assert_eq!(a.line_count, b.line_count);
}

#[test]
fn file_context_multiline_content() {
    let content = (0..100).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
    let c = FileContext {
        path: PathBuf::from("/multi"),
        content: content.clone(),
        line_count: 100,
    };
    assert_eq!(c.line_count, 100);
    assert!(c.content.contains("line 50"));
}
