//! Integration tests for sisyphus-tools-system `file` module.
//!
//! Exercises the public `read` / `read_lines` / `write` / `edit` /
//! `edit_all` / `glob` / `grep` / `search_patterns` / `gather_context`
//! functions — the building blocks for file manipulation tools.

use sisyphus_tools_system::file::{
    edit, edit_all, gather_context, glob, grep, read, read_lines, search_patterns, write,
};
use std::fs;
use tempfile::TempDir;

fn write_file(dir: &std::path::Path, rel: &str, body: &str) -> std::path::PathBuf {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(&p, body).expect("write");
    p
}

// ---------------------------------------------------------------------------
// read / write
// ---------------------------------------------------------------------------

#[test]
fn write_and_read_roundtrip() {
    let dir = TempDir::new().expect("tempdir");
    let p = dir.path().join("a.txt");
    write(&p, "hello").expect("write");
    let content = read(&p).expect("read");
    assert_eq!(content, "hello");
}

#[test]
fn read_nonexistent_returns_not_found_error() {
    let dir = TempDir::new().expect("tempdir");
    let p = dir.path().join("nope.txt");
    let result = read(&p);
    assert!(result.is_err());
}

#[test]
fn write_creates_parent_dirs() {
    let dir = TempDir::new().expect("tempdir");
    let p = dir.path().join("a").join("b").join("c.txt");
    write(&p, "nested").expect("write");
    assert!(p.exists());
    assert_eq!(read(&p).expect("read"), "nested");
}

// ---------------------------------------------------------------------------
// read_lines
// ---------------------------------------------------------------------------

#[test]
fn read_lines_with_offset_and_limit() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "lines.txt", "l1\nl2\nl3\nl4\nl5\n");
    // Skip 2 lines, take 2
    let result = read_lines(&p, 2, 2).expect("read_lines");
    assert_eq!(result, "l3\nl4");
}

#[test]
fn read_lines_offset_zero() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "lines.txt", "a\nb\nc\n");
    let result = read_lines(&p, 0, 2).expect("read_lines");
    assert_eq!(result, "a\nb");
}

#[test]
fn read_lines_limit_exceeds_remaining() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "lines.txt", "a\nb\n");
    let result = read_lines(&p, 0, 100).expect("read_lines");
    assert_eq!(result, "a\nb");
}

#[test]
fn read_lines_offset_past_end() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "lines.txt", "a\nb\n");
    let result = read_lines(&p, 100, 10).expect("read_lines");
    assert_eq!(result, "");
}

// ---------------------------------------------------------------------------
// edit / edit_all
// ---------------------------------------------------------------------------

#[test]
fn edit_replaces_occurrence() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "a.txt", "hello world\nhello again\n");
    edit(&p, "hello", "HI").expect("edit");
    let after = read(&p).expect("read");
    // Default edit only replaces the first occurrence.
    assert_eq!(after, "HI world\nhello again\n");
}

#[test]
fn edit_replaces_all_when_requested() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "a.txt", "foo\nfoo\nfoo\n");
    let n = edit_all(&p, "foo", "bar").expect("edit_all");
    assert_eq!(n, 3);
    let after = read(&p).expect("read");
    assert_eq!(after, "bar\nbar\nbar\n");
}

#[test]
fn edit_string_not_present_does_not_change_file() {
    let dir = TempDir::new().expect("tempdir");
    let original = "abc\ndef\n";
    let p = write_file(dir.path(), "a.txt", original);
    let result = edit(&p, "xyz", "123");
    // No occurrence — typically errors or is a no-op.
    // Just check the file is either unchanged or an error.
    let after = read(&p).expect("read");
    if result.is_ok() {
        assert_eq!(after, original);
    }
}

#[test]
fn edit_all_returns_count_of_replacements() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "a.txt", "x x x x x");
    let n = edit_all(&p, "x", "y").expect("edit_all");
    assert_eq!(n, 5);
}

// ---------------------------------------------------------------------------
// glob
// ---------------------------------------------------------------------------

#[test]
fn glob_finds_matching_files() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "a.rs", "");
    write_file(dir.path(), "b.rs", "");
    write_file(dir.path(), "c.txt", "");
    let pattern = format!("{}/**/*.rs", dir.path().display());
    let paths = glob(&pattern, dir.path()).expect("glob");
    // The pattern matches all .rs files.
    let names: Vec<String> = paths
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    assert!(names.contains(&"a.rs".to_string()));
    assert!(names.contains(&"b.rs".to_string()));
    assert!(!names.contains(&"c.txt".to_string()));
}

#[test]
fn glob_invalid_pattern_returns_error() {
    let dir = TempDir::new().expect("tempdir");
    // Unclosed bracket is an invalid glob pattern.
    let result = glob("nonexistent_dir/**/*[", dir.path());
    assert!(result.is_err());
}

#[test]
fn glob_caps_at_100_results() {
    let dir = TempDir::new().expect("tempdir");
    for i in 0..150 {
        write_file(dir.path(), &format!("file_{i}.txt"), "");
    }
    let pattern = format!("{}/**/*.txt", dir.path().display());
    let paths = glob(&pattern, dir.path()).expect("glob");
    assert!(paths.len() <= 100, "got {} paths", paths.len());
}

// ---------------------------------------------------------------------------
// grep
// ---------------------------------------------------------------------------

#[test]
fn grep_finds_matching_lines() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "a.rs", "alpha\nbeta\ngamma\n");
    let matches = grep("alpha", &p, None).expect("grep");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line, 1);
    assert!(matches[0].content.contains("alpha"));
}

#[test]
fn grep_multiple_matches() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "a.rs", "foo\nbar\nfoo\nbaz\nfoo\n");
    let matches = grep("foo", &p, None).expect("grep");
    assert_eq!(matches.len(), 3);
    assert_eq!(matches[0].line, 1);
    assert_eq!(matches[1].line, 3);
    assert_eq!(matches[2].line, 5);
}

#[test]
fn grep_invalid_regex_returns_error() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "a.rs", "x");
    let result = grep("(unclosed", &p, None);
    assert!(result.is_err());
}

#[test]
fn grep_recurses_directory() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "a.rs", "match_here");
    write_file(dir.path(), "sub/b.rs", "match_here");
    let matches = grep("match_here", dir.path(), None).expect("grep");
    assert_eq!(matches.len(), 2);
}

#[test]
fn grep_with_include_filter() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "a.rs", "match");
    write_file(dir.path(), "b.txt", "match");
    let matches = grep("match", dir.path(), Some(".rs")).expect("grep");
    // Only .rs files contain ".rs" in their path.
    assert_eq!(matches.len(), 1);
    assert!(matches[0].file.to_string_lossy().ends_with(".rs"),
        "got {:?}",
        matches[0].file
    );
}

// ---------------------------------------------------------------------------
// search_patterns / gather_context
// ---------------------------------------------------------------------------

#[test]
fn file_search_patterns_includes_context() {
    let dir = TempDir::new().expect("tempdir");
    write_file(
        dir.path(),
        "a.rs",
        "line1\nline2\nTARGET line3\nline4\nline5\n",
    );
    let r = search_patterns(&["TARGET"], dir.path(), None, 1).expect("ok");
    assert!(!r.matches.is_empty());
    let m = &r.matches[0];
    assert_eq!(m.before.len(), 1);
    assert_eq!(m.before[0], "line2");
    assert_eq!(m.after.len(), 1);
    assert_eq!(m.after[0], "line4");
}

#[test]
fn file_gather_context_returns_existing_files() {
    let dir = TempDir::new().expect("tempdir");
    let p = write_file(dir.path(), "a.rs", "first\nsecond\n");
    let ctx = gather_context(&[p.as_path()]).expect("ok");
    assert_eq!(ctx.len(), 1);
    assert!(ctx[0].content.contains("second"));
    assert_eq!(ctx[0].line_count, 2);
}
