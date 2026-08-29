//! Integration tests for sisyphus-tools-system `explore` module's
//! pure helper: `search_patterns` with empty patterns (early return path).

use sisyphus_tools_system::explore::search_patterns;
use std::path::Path;

#[test]
fn search_patterns_empty_patterns_returns_empty() {
    let r = search_patterns(&[], Path::new("/tmp"), None, 0).unwrap();
    assert!(r.matches.is_empty());
    assert_eq!(r.files_scanned, 0);
    assert_eq!(r.total_matches, 0);
}

#[test]
fn search_patterns_empty_with_include() {
    let r = search_patterns(&[], Path::new("/tmp"), Some("*.rs"), 0).unwrap();
    assert!(r.matches.is_empty());
}

#[test]
fn search_patterns_empty_with_context() {
    let r = search_patterns(&[], Path::new("/tmp"), None, 5).unwrap();
    assert!(r.matches.is_empty());
    assert_eq!(r.total_matches, 0);
}

#[test]
fn search_patterns_empty_with_invalid_root() {
    // Even with non-existent root, empty patterns short-circuits
    let r = search_patterns(&[], Path::new("/nonexistent/abc/def"), None, 0).unwrap();
    assert!(r.matches.is_empty());
}

#[test]
fn search_patterns_empty_with_all_options() {
    let r = search_patterns(&[], Path::new("/"), Some("*.toml"), 10).unwrap();
    assert!(r.matches.is_empty());
    assert_eq!(r.files_scanned, 0);
}
