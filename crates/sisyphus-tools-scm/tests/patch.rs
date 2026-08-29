//! Integration tests for sisyphus-tools-scm `apply_patch`.
//!
//! Exercises the public `apply_patch` API which applies a unified diff
//! string to a file on disk. Verifies the standard diff -u format
//! behaviors:
//! - simple line replacement
//! - line removal
//! - line addition
//! - context-mismatch detection
//! - multiple hunks
//! - empty / malformed input
//! - trailing-newline preservation
//!
//! These tests run against the public `apply_patch` entry point and
//! complement the in-crate tests that exercise internal helpers.

use sisyphus_tools_scm::patch::apply_patch;
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

#[test]
fn apply_patch_replaces_line() {
    let dir = TempDir::new().expect("tempdir");
    let f = write_file(dir.path(), "a.rs", "fn main() {\n    old();\n}\n");
    let patch = "\
--- a/a.rs
+++ b/a.rs
@@ -1,3 +1,3 @@
 fn main() {
-    old();
+    new();
 }
";
    let modified = apply_patch(&f, patch).expect("patch");
    assert_eq!(modified.len(), 1);
    assert_eq!(fs::read_to_string(&f).expect("read"), "fn main() {\n    new();\n}\n");
}

#[test]
fn apply_patch_adds_line() {
    let dir = TempDir::new().expect("tempdir");
    let f = write_file(dir.path(), "a.txt", "line1\n");
    let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1,1 +1,2 @@
 line1
+line2
";
    apply_patch(&f, patch).expect("patch");
    assert_eq!(fs::read_to_string(&f).expect("read"), "line1\nline2\n");
}

#[test]
fn apply_patch_removes_line() {
    let dir = TempDir::new().expect("tempdir");
    let f = write_file(dir.path(), "a.txt", "line1\nline2\nline3\n");
    let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,2 @@
 line1
-line2
 line3
";
    apply_patch(&f, patch).expect("patch");
    assert_eq!(fs::read_to_string(&f).expect("read"), "line1\nline3\n");
}

#[test]
fn apply_patch_multiple_hunks() {
    let dir = TempDir::new().expect("tempdir");
    let f = write_file(
        dir.path(),
        "a.txt",
        "old1\nold2\nold3\nold4\nold5\nold6\n",
    );
    // Two hunks, modifying lines 1 and 5.
    let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1,1 +1,1 @@
-old1
+new1
@@ -5,1 +5,1 @@
-old5
+new5
";
    apply_patch(&f, patch).expect("patch");
    assert_eq!(
        fs::read_to_string(&f).expect("read"),
        "new1\nold2\nold3\nold4\nnew5\nold6\n"
    );
}

#[test]
fn apply_patch_context_mismatch_fails() {
    let dir = TempDir::new().expect("tempdir");
    let f = write_file(dir.path(), "a.txt", "xxx\nccc\n");
    let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1,1 +1,1 @@
-aaa
+bbb
";
    let result = apply_patch(&f, patch);
    assert!(result.is_err());
    let msg = format!("{result:?}");
    assert!(msg.contains("mismatch"), "got: {msg}");
}

#[test]
fn apply_patch_empty_patch_fails() {
    let dir = TempDir::new().expect("tempdir");
    let f = write_file(dir.path(), "a.txt", "anything\n");
    let result = apply_patch(&f, "");
    assert!(result.is_err());
    let msg = format!("{result:?}");
    assert!(msg.contains("empty") || msg.contains("no hunks"), "got: {msg}");
}

#[test]
fn apply_patch_preserves_trailing_newline() {
    let dir = TempDir::new().expect("tempdir");
    let f = write_file(dir.path(), "a.txt", "old\n");
    let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1,1 +1,1 @@
-old
+new
";
    apply_patch(&f, patch).expect("patch");
    // Trailing newline is preserved.
    assert_eq!(fs::read_to_string(&f).expect("read"), "new\n");
}

#[test]
fn apply_patch_no_trailing_newline_input() {
    let dir = TempDir::new().expect("tempdir");
    let f = write_file(dir.path(), "a.txt", "old");
    let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1,1 +1,1 @@
-old
+new
";
    apply_patch(&f, patch).expect("patch");
    // No trailing newline in the input means no trailing newline in output.
    assert_eq!(fs::read_to_string(&f).expect("read"), "new");
}

#[test]
fn apply_patch_malformed_hunk_header_fails() {
    let dir = TempDir::new().expect("tempdir");
    let f = write_file(dir.path(), "a.txt", "x\n");
    // Hunk header missing the second range token.
    let patch = "@@ -1 @@\n";
    let result = apply_patch(&f, patch);
    assert!(result.is_err());
}

#[test]
fn apply_patch_returns_modified_file_path() {
    let dir = TempDir::new().expect("tempdir");
    let f = write_file(dir.path(), "sub/dir/file.txt", "x\n");
    let patch = "\
--- a/sub/dir/file.txt
+++ b/sub/dir/file.txt
@@ -1,1 +1,1 @@
-x
+y
";
    let modified = apply_patch(&f, patch).expect("patch");
    assert_eq!(modified.len(), 1);
    // The returned path should mention the modified file.
    assert!(modified[0].contains("file.txt"));
}
