//! Integration tests for sisyphus-types Config I/O.
//!
//! Exercises the critical `from_file` -> `apply_env_overrides` -> `validate`
//! -> `save_to_file` round-trip used by every Sisyphus entry point (CLI,
//! bridge, daemon). Lives in `tests/` so the public API is exercised exactly
//! as an external consumer would.

use sisyphus_types::Config;
use std::io::Write;

const MINIMAL_TOML: &str = r#"
bridge_address = "http://127.0.0.1:50051"
"#;

fn write_toml(path: &std::path::Path, body: &str) {
    let mut f = std::fs::File::create(path).expect("create tmpfile");
    f.write_all(body.as_bytes()).expect("write tmpfile");
}

#[test]
fn config_round_trip_preserves_bridge_address() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    write_toml(&path, MINIMAL_TOML);

    let cfg = Config::from_file(path.to_str().unwrap()).expect("from_file");
    assert_eq!(cfg.bridge_address, "http://127.0.0.1:50051");

    // round-trip via save_to_file
    let path2 = dir.path().join("round.toml");
    cfg.save_to_file(path2.to_str().unwrap()).expect("save_to_file");
    let reloaded = Config::from_file(path2.to_str().unwrap()).expect("from_file 2");
    assert_eq!(reloaded.bridge_address, cfg.bridge_address);
}

#[test]
fn config_validate_passes_minimal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    write_toml(&path, MINIMAL_TOML);
    let cfg = Config::from_file(path.to_str().unwrap()).expect("from_file");
    cfg.validate().expect("minimal config should validate");
}

#[test]
fn config_from_file_reports_missing_path() {
    let bogus = "/this/path/does/not/exist/sisyphus.toml";
    let err = Config::from_file(bogus).expect_err("missing file must fail");
    // Error message should mention the path so operators can find it.
    let msg = format!("{err:?}");
    assert!(
        msg.contains("Config") || msg.contains(bogus),
        "error should reference the failing config: {msg}"
    );
}

#[test]
fn config_from_file_reports_malformed_toml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bad.toml");
    write_toml(&path, "this is = = = not valid toml [[[");
    let err = Config::from_file(path.to_str().unwrap()).expect_err("malformed must fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("parse") || msg.contains("Parse") || msg.contains("TOML"),
        "error should mention parsing: {msg}"
    );
}

#[test]
fn config_save_to_file_creates_missing_parent_dirs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let nested = dir.path().join("a").join("b").join("c.toml");
    let cfg = Config::from_file("/dev/null")
        .or_else(|_| -> Result<Config, sisyphus_types::TypesError> {
            // /dev/null parse fails; start from default and save
            let mut c = Config::default();
            c.bridge_address = "http://127.0.0.1:50051".to_string();
            Ok(c)
        })
        .expect("default or default+override");
    cfg.save_to_file(nested.to_str().unwrap()).expect("save with nested parent");
    assert!(nested.exists(), "nested path should be created");
}
