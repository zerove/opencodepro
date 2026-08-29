//! Integration tests for sisyphus-tools-build `AutoFixer`.
//!
//! Exercises the public `AutoFixer::parse_errors`, `suggest_fix`, and
//! `analyze_and_suggest` API. These functions parse rustc compiler
//! errors and produce human-readable fix suggestions with a confidence
//! score.

use sisyphus_tools_build::auto_fix::{AutoFixer, ParsedError};

// ---------------------------------------------------------------------------
// parse_errors
// ---------------------------------------------------------------------------

#[test]
fn parse_errors_empty_input_returns_empty() {
    let errs = AutoFixer::parse_errors("");
    assert!(errs.is_empty());
}

#[test]
fn parse_errors_handles_error_with_code_brackets() {
    let output = "error[E0308]: mismatched types\n  --> src/foo.rs:42:12\n";
    let errs = AutoFixer::parse_errors(output);
    assert_eq!(errs.len(), 1);
    let e = &errs[0];
    assert_eq!(e.error_code, "E0308");
    assert!(e.message.contains("mismatched types"));
    assert_eq!(e.file.as_deref(), Some("src/foo.rs"));
    assert_eq!(e.line, 42);
    assert_eq!(e.column, 12);
}

#[test]
fn parse_errors_handles_error_without_code() {
    let output = "error: cannot find value `foo` in this scope\n  --> src/bar.rs:10:5\n";
    let errs = AutoFixer::parse_errors(output);
    assert_eq!(errs.len(), 1);
    let e = &errs[0];
    // No code → fallback "E0000"
    assert_eq!(e.error_code, "E0000");
    assert!(e.message.contains("cannot find value"));
    assert_eq!(e.file.as_deref(), Some("src/bar.rs"));
    assert_eq!(e.line, 10);
    assert_eq!(e.column, 5);
}

#[test]
fn parse_errors_handles_multiple_errors() {
    let output = "\
error[E0308]: mismatched types
  --> src/a.rs:1:2

error[E0432]: unresolved import
  --> src/b.rs:3:4
";
    let errs = AutoFixer::parse_errors(output);
    assert_eq!(errs.len(), 2);
    assert_eq!(errs[0].error_code, "E0308");
    assert_eq!(errs[1].error_code, "E0432");
    assert_eq!(errs[0].file.as_deref(), Some("src/a.rs"));
    assert_eq!(errs[1].file.as_deref(), Some("src/b.rs"));
}

#[test]
fn parse_errors_ignores_non_error_lines() {
    let output = "\
warning: unused variable
  --> src/x.rs:5:1

note: previous definition

error[E0001]: actual error
  --> src/y.rs:7:8
";
    let errs = AutoFixer::parse_errors(output);
    // Only the error[E0001] line creates a ParsedError; warning/note are skipped.
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].error_code, "E0001");
}

#[test]
fn parse_errors_file_with_no_extension() {
    let output = "error[E0001]: something\n  --> path/to/file:5:5\n";
    let errs = AutoFixer::parse_errors(output);
    assert_eq!(errs[0].file.as_deref(), Some("path/to/file"));
}

#[test]
fn parse_errors_malformed_column_defaults_to_zero() {
    let output = "error[E0001]: something\n  --> src/x.rs:5:notanumber\n";
    let errs = AutoFixer::parse_errors(output);
    assert_eq!(errs[0].column, 0);
    assert_eq!(errs[0].line, 5);
}

// ---------------------------------------------------------------------------
// suggest_fix
// ---------------------------------------------------------------------------

#[test]
fn suggest_fix_e0308_type_mismatch() {
    let err = ParsedError {
        error_code: "E0308".into(),
        message: "mismatched types".into(),
        file: Some("src/foo.rs".into()),
        line: 42,
        column: 12,
    };
    let fix = AutoFixer::suggest_fix(&err).expect("fix");
    assert_eq!(fix.error_code, "E0308");
    assert!(fix.suggested_fix.contains("Type mismatch"));
    assert!(fix.suggested_fix.contains("src/foo.rs"));
    assert!((fix.confidence - 0.5).abs() < 1e-9);
}

#[test]
fn suggest_fix_e0425_cannot_find_value() {
    let err = ParsedError {
        error_code: "E0425".into(),
        message: "cannot find value `foo` in this scope".into(),
        file: Some("src/x.rs".into()),
        line: 7,
        column: 0,
    };
    let fix = AutoFixer::suggest_fix(&err).expect("fix");
    assert!(fix.suggested_fix.contains("Cannot find"));
    assert!((fix.confidence - 0.4).abs() < 1e-9);
}

#[test]
fn suggest_fix_e0000_cannot_find_value() {
    // E0000 with "cannot find" message → uses the cannot-find branch.
    let err = ParsedError {
        error_code: "E0000".into(),
        message: "cannot find type `Bar`".into(),
        file: None,
        line: 0,
        column: 0,
    };
    let fix = AutoFixer::suggest_fix(&err).expect("fix");
    assert!(fix.suggested_fix.contains("Cannot find"));
}

#[test]
fn suggest_fix_e0432_unresolved_import() {
    let err = ParsedError {
        error_code: "E0432".into(),
        message: "unresolved import `foo`".into(),
        file: Some("src/lib.rs".into()),
        line: 1,
        column: 0,
    };
    let fix = AutoFixer::suggest_fix(&err).expect("fix");
    assert!(fix.suggested_fix.contains("Unresolved import"));
    assert!((fix.confidence - 0.6).abs() < 1e-9);
}

#[test]
fn suggest_fix_e0277_trait_bound() {
    let err = ParsedError {
        error_code: "E0277".into(),
        message: "trait bound not satisfied".into(),
        file: Some("src/x.rs".into()),
        line: 1,
        column: 0,
    };
    let fix = AutoFixer::suggest_fix(&err).expect("fix");
    assert!(fix.suggested_fix.contains("Trait bound"));
}

#[test]
fn suggest_fix_e0599_no_method() {
    let err = ParsedError {
        error_code: "E0599".into(),
        message: "no method named `foo`".into(),
        file: None,
        line: 0,
        column: 0,
    };
    let fix = AutoFixer::suggest_fix(&err).expect("fix");
    assert!(fix.suggested_fix.contains("No method named"));
}

#[test]
fn suggest_fix_unknown_error_code_returns_fallback() {
    let err = ParsedError {
        error_code: "E9999".into(),
        message: "very obscure error".into(),
        file: Some("src/x.rs".into()),
        line: 99,
        column: 1,
    };
    let fix = AutoFixer::suggest_fix(&err).expect("always returns Some");
    assert!(fix.suggested_fix.contains("Unrecognized"));
    assert!(fix.suggested_fix.contains("E9999"));
    // Lower confidence for unknown errors.
    assert!(fix.confidence < 0.5);
}

#[test]
fn suggest_fix_preserves_original_error_metadata() {
    let err = ParsedError {
        error_code: "E0308".into(),
        message: "expected i32, found String".into(),
        file: Some("a.rs".into()),
        line: 11,
        column: 22,
    };
    let fix = AutoFixer::suggest_fix(&err).expect("fix");
    assert_eq!(fix.file, "a.rs");
    assert_eq!(fix.line, 11);
    assert_eq!(fix.column, 22);
    assert_eq!(fix.error_message, "expected i32, found String");
}

// ---------------------------------------------------------------------------
// analyze_and_suggest
// ---------------------------------------------------------------------------

#[test]
fn analyze_and_suggest_returns_one_fix_per_error() {
    let output = "\
error[E0308]: mismatched types
  --> src/a.rs:1:1

error[E0432]: unresolved import
  --> src/b.rs:2:2
";
    let suggestions = AutoFixer::analyze_and_suggest(output, "src/a.rs");
    assert_eq!(suggestions.len(), 2);
    assert_eq!(suggestions[0].error_code, "E0308");
    assert_eq!(suggestions[1].error_code, "E0432");
}

#[test]
fn analyze_and_suggest_empty_output_returns_empty() {
    let suggestions = AutoFixer::analyze_and_suggest("", "any.rs");
    assert!(suggestions.is_empty());
}
