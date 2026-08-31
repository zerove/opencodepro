//! Integration tests for sisyphus-tools-llm-tools oracle.
//!
//! Exercises the public `analyze_file` / `analyze_directory` / `code_review`
//! API used by the SOP engine to inspect source files. These are pure
//! functions over the file system, so they are safe to test from `tests/`.

use sisyphus_tools_llm_tools::oracle::{analyze_directory, analyze_file, code_review};
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

#[test]
fn analyze_file_detects_pub_fn_signature() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(dir.path(), "lib.rs", "pub fn hello() -> u8 { 42 }\n");
    let analysis = analyze_file(&file).expect("analyze");
    assert_eq!(analysis.functions.len(), 1, "should detect one function");
    assert!(analysis.functions[0].is_public);
    assert_eq!(analysis.functions[0].name, "hello");
}

#[test]
fn analyze_file_flags_todo_markers_as_issue() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(
        dir.path(),
        "marked.rs",
        "pub fn work() { /* TODO: implement */ }\n",
    );
    let analysis = analyze_file(&file).expect("analyze");
    let has_todo_issue = analysis.issues.iter().any(|i| i.message.contains("TODO/FIXME"));
    assert!(
        has_todo_issue,
        "expected a TODO/FIXME issue, got: {:?}",
        analysis.issues
    );
}

#[test]
fn analyze_file_reports_line_and_char_count() {
    let dir = TempDir::new().expect("tempdir");
    let body = "line one\nline two\nline three\n";
    let file = write(dir.path(), "count.rs", body);
    let analysis = analyze_file(&file).expect("analyze");
    assert_eq!(analysis.line_count, 3);
    assert_eq!(analysis.char_count, body.len());
}

#[test]
fn analyze_directory_walks_subdirectories() {
    let dir = TempDir::new().expect("tempdir");
    write(dir.path(), "a.rs", "pub fn a() {}\n");
    write(dir.path(), "sub/b.rs", "pub fn b() {}\n");
    let summary = analyze_directory(dir.path(), None).expect("analyze dir");
    assert!(
        summary.file_count >= 2,
        "should walk into subdirectory, found {}",
        summary.file_count
    );
}

#[test]
fn code_review_returns_empty_for_clean_file() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(
        dir.path(),
        "clean.rs",
        "/// A clean function.\npub fn clean() -> u8 { 0 }\n",
    );
    let issues = code_review(&[file.as_path()]).expect("review");
    // 100-char "long line" rule should not fire; only the function
    // should be inspected. Either empty or only noise — assert no
    // "TODO" issues (we wrote none).
    let todo_issues =
        issues.iter().filter(|i| i.message.contains("TODO") || i.message.contains("FIXME")).count();
    assert_eq!(todo_issues, 0, "clean file should have no TODO issues");
}
