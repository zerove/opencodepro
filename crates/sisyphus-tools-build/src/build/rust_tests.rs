use super::*;
use std::fs;
use tempfile::TempDir;

fn write_toml(dir: &Path, content: &str) {
    // Ensure standalone project: append empty [workspace] so parent workspace
    // (e.g. .jcode/Cargo.toml) doesn't interfere with cargo commands.
    let toml = if content.contains("[workspace]") {
        content.to_string()
    } else {
        format!("{content}\n[workspace]\n")
    };
    fs::write(dir.join("Cargo.toml"), toml).unwrap();
}

fn write_src(dir: &Path, content: &str) {
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), content).unwrap();
}

#[test]
fn test_parse_test_results_all_ok() {
    let stdout = "\n     Running unittests src/lib.rs\n\nrunning 3 tests\ntest a ... ok\ntest b ... ok\ntest c ... ok\n\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\n";
    let r = parse_test_results(stdout, "");
    assert_eq!(r.passed, 3);
    assert_eq!(r.failed, 0);
    assert!(r.failures.is_empty());
}

#[test]
fn test_parse_test_results_with_failures() {
    let stdout = "running 2 tests\ntest a ... ok\ntest b ... FAILED\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored\n\n--- Failures ---\n\nfailures:\n    my_test::test_b\n\n";
    let r = parse_test_results(stdout, "");
    assert_eq!(r.passed, 1);
    assert_eq!(r.failed, 1);
    assert_eq!(r.failures.len(), 1);
    assert_eq!(r.failures[0].name, "my_test::test_b");
}

#[test]
fn test_parse_test_results_with_ignored() {
    let stdout = "test result: ok. 512 passed; 0 failed; 4 ignored; 0 measured; 0 filtered out\n";
    let r = parse_test_results(stdout, "");
    assert_eq!(r.passed, 512);
    assert_eq!(r.failed, 0);
    assert_eq!(r.ignored, 4);
}

#[test]
fn test_extract_diags_errors() {
    let stderr = "error[E0425]: cannot find value `x` in this scope\n  --> src/lib.rs:10:5\n   |\n10 |     let y = x;\n   |             ^ not found\n\nerror: aborting due to 1 previous error\n";
    let (errors, warnings) = extract_diags(stderr);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code.as_deref(), Some("E0425"));
    assert_eq!(errors[0].message, "cannot find value `x` in this scope");
    assert_eq!(errors[0].file.as_deref(), Some("src/lib.rs"));
    assert_eq!(errors[0].line, Some(10));
    assert_eq!(errors[0].col, Some(5));
    assert!(warnings.is_empty());
}

#[test]
fn test_extract_diags_with_ansi_colors() {
    // CI（dtolnay/rust-toolchain）设置 CARGO_TERM_COLOR=always，cargo 输出带 ANSI 色码
    let stderr = "\u{1b}[1m\u{1b}[92m    Checking\u{1b}[0m bad-crate\n\u{1b}[1m\u{1b}[91merror[E0425]\u{1b}[0m\u{1b}[1m: cannot find value `does_not_exist` in this scope\u{1b}[0m\n \u{1b}[1m\u{1b}[94m--> \u{1b}[0msrc/lib.rs:1:34\n  \u{1b}[1m\u{1b}[94m|\u{1b}[0m\n1 | pub fn hello() -> &'static str { does_not_exist }\n  \u{1b}[1m\u{1b}[91m^^^^^^^^^^^^^^\u{1b}[0m \u{1b}[1m\u{1b}[91mnot found in this scope\u{1b}[0m\n\n\u{1b}[1m\u{1b}[91merror\u{1b}[0m\u{1b}[1m:\u{1b}[0m could not compile `bad-crate` (lib) due to 1 previous error\n";
    let (errors, warnings) = extract_diags(stderr);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code.as_deref(), Some("E0425"));
    assert_eq!(
        errors[0].message,
        "cannot find value `does_not_exist` in this scope"
    );
    assert_eq!(errors[0].file.as_deref(), Some("src/lib.rs"));
    assert_eq!(errors[0].line, Some(1));
    assert_eq!(errors[0].col, Some(34));
    assert!(warnings.is_empty());
}

#[test]
fn test_extract_diags_warnings() {
    let stderr = "warning: unused variable `x`\n  --> src/lib.rs:5:9\n   |\n5 |     let x = 1;\n   |         ^ help: if you intend to use it: `_x`\n";
    let (errors, warnings) = extract_diags(stderr);
    assert!(errors.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].level, "warning");
    assert_eq!(warnings[0].file.as_deref(), Some("src/lib.rs"));
    assert_eq!(warnings[0].line, Some(5));
    assert_eq!(warnings[0].col, Some(9));
}

#[test]
fn test_extract_diags_multiple() {
    let stderr = "error[E0308]: mismatched types\n  --> src/main.rs:20:15\n   |\n20 |     let x: i32 = \"hello\";\n   |                  ^^^^^^^ expected i32, found &str\n\nwarning: unused import\n  --> src/main.rs:1:1\n   |\n1 | use std::collections::HashMap;\n   |\n";
    let (errors, warnings) = extract_diags(stderr);
    assert_eq!(errors.len(), 1);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn test_check_on_valid_project() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_src(path, "pub fn hello() -> &'static str { \"hello\" }\n");
    let backend = RustBackend;
    let result = backend.check(None, false, Some(path)).unwrap();
    assert!(result.success);
    assert!(result.errors.is_empty());
}

#[test]
fn test_check_on_invalid_project() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "bad-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_src(path, "pub fn hello() -> &'static str { does_not_exist }\n");
    let backend = RustBackend;
    let result = backend.check(None, false, Some(path)).unwrap();
    assert!(!result.success);
    assert!(!result.errors.is_empty());
}

#[test]
fn test_fmt_check_formatted() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "fmt-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_src(path, "pub fn foo() -> u8 {\n    42\n}\n");
    let backend = RustBackend;
    let ok = backend.fmt_check(Some(path)).unwrap();
    assert!(ok);
}

#[test]
fn test_fmt_check_unformatted() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "bad-fmt-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_src(path, "pub fn foo() -> u8 { 42 }\n");
    let backend = RustBackend;
    // First check it
    backend.fmt_check(Some(path)).unwrap();
    // Then break formatting
    fs::write(path.join("src/lib.rs"), "pub fn foo() -> u8 {\n42\n}\n").unwrap();
    let ok = backend.fmt_check(Some(path)).unwrap();
    assert!(!ok);
}

#[test]
fn test_build_valid() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "build-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_src(path, "pub fn works() -> i32 { 1 + 2 }\n");
    let backend = RustBackend;
    let result = backend.build(None, false, Some(path)).unwrap();
    assert!(result.success);
}

#[test]
fn test_test_simple() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    let src = path.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
#[test]
fn it_works() { assert_eq!(2 + 2, 4); }

#[test]
fn it_also_works() { assert!(true); }
"#,
    )
    .unwrap();
    let backend = RustBackend;
    let results = backend.test(None, None, false, Some(path)).unwrap();
    assert_eq!(results.passed, 2);
    assert_eq!(results.failed, 0);
}

#[test]
fn test_test_with_failures() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "fail-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    let src = path.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
#[test]
fn passing() { assert!(true); }

#[test]
fn failing() { assert!(false, "intentional failure"); }
"#,
    )
    .unwrap();
    let backend = RustBackend;
    let results = backend.test(None, None, false, Some(path)).unwrap();
    assert_eq!(results.passed, 1);
    assert_eq!(results.failed, 1);
    assert_eq!(results.failures.len(), 1);
}

#[test]
fn test_backend_name() {
    let backend = RustBackend;
    assert_eq!(backend.name(), "rust");
}
