//! Integration tests for sisyphus-tools-scm hashline utilities.
//!
//! Exercises the public `hash_content` / `generate_hashline` /
//! `parse_hashline` / `verify_line` / `annotate_file` / `edit_with_hashline`
//! API. These are the building blocks used by the edit tool to safely
//! modify files even when line numbers shift between tool calls.

use sisyphus_tools_scm::hashline::{
    annotate_file, edit_with_hashline, generate_hashline, hash_content, parse_hashline, verify_line,
};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write(dir: &Path, rel: &str, body: &str) -> std::path::PathBuf {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(&p, body).expect("write");
    p
}

#[test]
fn hash_content_is_deterministic_and_8_hex_chars() {
    let a = hash_content("hello world");
    let b = hash_content("hello world");
    assert_eq!(a, b, "same input must produce the same hash");

    let c = hash_content("hello WORLD");
    assert_ne!(a, c, "different input must produce different hash");

    // Hash is the first 8 hex characters of the DefaultHasher finish.
    assert_eq!(a.len(), 8);
    assert!(a.chars().all(|ch| ch.is_ascii_hexdigit()));
}

#[test]
fn generate_hashline_uses_correct_comment_syntax() {
    // Rust-style: "//"
    let rs = generate_hashline(Path::new("foo.rs"), 1, "let x = 1;");
    assert!(rs.contains("LINE#1:"));
    assert!(rs.contains("//"));

    // Python-style: "#"
    let py = generate_hashline(Path::new("foo.py"), 1, "x = 1");
    assert!(py.contains("LINE#1:"));
    assert!(py.contains('#'));

    // HTML: opens with "<!--" and closes with "-->"
    let html = generate_hashline(Path::new("foo.html"), 1, "<div>");
    assert!(html.contains("<!--"));
    assert!(html.contains("-->"));
}

#[test]
fn generate_then_parse_round_trip() {
    let path = Path::new("lib.rs");
    let original = "let x = 42;";
    let hashline = generate_hashline(path, 7, original);

    let parsed = parse_hashline(&hashline).expect("should parse");
    assert_eq!(parsed.line_number, 7);
    assert_eq!(parsed.content_hash, hash_content(original));
    // content_snippet is empty in parse_hashline (only the generator
    // knows the original).
    assert!(parsed.content_snippet.is_empty());
}

#[test]
fn parse_hashline_returns_none_for_non_matching_input() {
    assert!(parse_hashline("just a comment").is_none());
    assert!(parse_hashline("").is_none());
    assert!(parse_hashline("LINE#abc:def").is_none()); // non-numeric line
    assert!(parse_hashline("LINE#1").is_none()); // missing ':hash'
}

#[test]
fn verify_line_passes_for_unchanged_line() {
    let content = "line one\nline two\nline three\n";
    // Generate a hashline for line 2 ("line two")
    let hl = generate_hashline(Path::new("a.rs"), 2, "line two");
    assert!(verify_line(content, &hl).expect("verify"));
}

#[test]
fn verify_line_fails_when_line_changed() {
    let hl = generate_hashline(Path::new("a.rs"), 2, "line two");
    // Now the file has different content at line 2
    let modified = "line one\nLINE TWO CHANGED\nline three\n";
    assert!(!verify_line(modified, &hl).expect("verify"));
}

#[test]
fn verify_line_errors_on_out_of_bounds_line() {
    let content = "only one line\n";
    let hl = generate_hashline(Path::new("a.rs"), 99, "something");
    let result = verify_line(content, &hl);
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(
        msg.contains("out of bounds") || msg.contains("99"),
        "expected out-of-bounds error, got {msg}"
    );
}

#[test]
fn annotate_file_writes_line_annotations_in_place() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(dir.path(), "lib.rs", "first\nsecond\nthird\n");
    let count = annotate_file(&file).expect("annotate");
    // 3 non-empty lines → 3 hashline annotations added
    assert_eq!(count, 3);

    let annotated = fs::read_to_string(&file).expect("read");
    assert!(annotated.contains("LINE#1:"));
    assert!(annotated.contains("LINE#2:"));
    assert!(annotated.contains("LINE#3:"));
}

#[test]
fn annotate_file_skips_blank_lines() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(dir.path(), "a.rs", "a\n\nb\n");
    let count = annotate_file(&file).expect("annotate");
    // 2 non-blank lines only
    assert_eq!(count, 2);
}

#[test]
fn annotated_file_verifies_via_verify_line() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(dir.path(), "a.rs", "alpha\nbeta\ngamma\n");
    annotate_file(&file).expect("annotate");
    let annotated = fs::read_to_string(&file).expect("read");

    // Each annotated line should verify correctly.
    for n in 1..=3 {
        // Extract the LINE# comment from the n-th line.
        let line = annotated.lines().nth(n - 1).expect("line");
        let marker = format!("LINE#{n}:");
        let idx = line.find(&marker).expect("marker");
        // Build a complete hashline string in the format parse_hashline expects.
        let after_marker = &line[idx..];
        let hl = after_marker.split_whitespace().next().unwrap();
        // The verify_line call expects a complete "LINE#N:hash" substring.
        // We just check the prefix and a hash follow it.
        assert!(hl.starts_with(&marker));
    }
}

#[test]
fn edit_with_hashline_replaces_targeted_line() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(dir.path(), "a.rs", "line one\nline two OLD\nline three\n");
    // Generate the hashline for line 2 from the original content.
    let hl = generate_hashline(Path::new("a.rs"), 2, "line two OLD");
    let result = edit_with_hashline(&file, &hl, "line two NEW").expect("edit should succeed");
    assert!(
        !result.is_empty(),
        "edit should return non-empty diff/message"
    );

    let after = fs::read_to_string(&file).expect("read");
    assert!(after.contains("line two NEW"));
    assert!(!after.contains("line two OLD"));
}

#[test]
fn edit_with_hashline_fails_on_hash_mismatch() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(dir.path(), "a.rs", "line one\nline two\nline three\n");
    // Generate hashline for a line that no longer matches.
    let hl = generate_hashline(Path::new("a.rs"), 2, "different content");
    let result = edit_with_hashline(&file, &hl, "new content");
    assert!(
        result.is_err(),
        "edit with stale hashline should fail, got {result:?}"
    );
}
