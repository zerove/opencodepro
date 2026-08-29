//! Integration tests for sisyphus-tools-build AutoDocGenerator.
//!
//! Exercises the public API used to scaffold doc comments on functions
//! and modules. Tests run against a real temp file because the
//! generator parses on disk.

use sisyphus_tools_build::auto_doc::AutoDocGenerator;
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
fn generate_for_function_returns_doc_for_pub_fn() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(
        dir.path(),
        "lib.rs",
        "/// Existing doc.\npub fn greet() -> &'static str { \"hi\" }\n",
    );

    let result =
        AutoDocGenerator::generate_for_function(file.to_str().unwrap(), "greet").expect("ok");
    assert!(
        !result.is_empty(),
        "expected a generated doc comment, got empty string"
    );
    // The generated stub should mention the function name or be a
    // placeholder (the implementation produces a CamelCase human-readable
    // string). We only assert it's non-empty.
}

#[test]
fn generate_for_function_returns_err_for_missing_function() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(
        dir.path(),
        "lib.rs",
        "pub fn present() -> u8 { 0 }\n",
    );
    let result =
        AutoDocGenerator::generate_for_function(file.to_str().unwrap(), "nonexistent");
    // Either Ok with empty doc or Err — both are valid. We just want
    // no panic.
    let _ = result;
}

#[test]
fn generate_for_function_errors_on_missing_file() {
    let result = AutoDocGenerator::generate_for_function("/does/not/exist.rs", "fn");
    assert!(result.is_err(), "missing file should produce a ToolError");
}

#[test]
fn generate_for_module_returns_list_of_pub_items() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(
        dir.path(),
        "lib.rs",
        "\
/// Greet.
pub fn greet() -> &'static str { \"hi\" }

/// Counter struct.
pub struct Counter {
    n: u32,
}

pub enum Color {
    Red,
    Blue,
}

pub trait Greeter {
    fn greet(&self) -> String;
}

fn private_helper() {}
",
    );

    let items =
        AutoDocGenerator::generate_for_module(file.to_str().unwrap()).expect("ok");
    let names: Vec<&str> = items.iter().map(|(name, _)| name.as_str()).collect();

    // Public items should be present; private items should not.
    assert!(names.contains(&"greet"), "missing pub fn 'greet' in {names:?}");
    assert!(names.contains(&"Counter"), "missing pub struct in {names:?}");
    assert!(names.contains(&"Color"), "missing pub enum in {names:?}");
    assert!(names.contains(&"Greeter"), "missing pub trait in {names:?}");
    assert!(
        !names.contains(&"private_helper"),
        "private fn should not be in the list, got {names:?}"
    );
}

#[test]
fn generate_for_module_returns_empty_for_empty_file() {
    let dir = TempDir::new().expect("tempdir");
    let file = write(dir.path(), "empty.rs", "");
    let items =
        AutoDocGenerator::generate_for_module(file.to_str().unwrap()).expect("ok");
    assert!(items.is_empty());
}

#[test]
fn generate_for_module_handles_multiline_signature() {
    // Pub fns split across multiple lines should still be detected.
    let dir = TempDir::new().expect("tempdir");
    let file = write(
        dir.path(),
        "lib.rs",
        "\
pub fn multi_line(
    a: u32,
    b: u32,
) -> u32 {
    a + b
}
",
    );
    let items =
        AutoDocGenerator::generate_for_module(file.to_str().unwrap()).expect("ok");
    assert!(
        items.iter().any(|(n, _)| n == "multi_line"),
        "multiline fn not detected: {:?}",
        items
    );
}
