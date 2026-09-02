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
            let c = Config {
                bridge_address: "http://127.0.0.1:50051".to_string(),
                ..Config::default()
            };
            Ok(c)
        })
        .expect("default or default+override");
    cfg.save_to_file(nested.to_str().unwrap()).expect("save with nested parent");
    assert!(nested.exists(), "nested path should be created");
}

// ── mutation testing 防护 ──
// cargo-mutants 抓"把 save_to_file 替换为 Ok(())"会绕过 round-trip 测试
// （如果 cfg 是 default 的话）。需要验证**非默认配置**能真正写入并读出。
#[test]
fn config_save_to_file_writes_non_default_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("non_default.toml");

    let cfg = Config {
        bridge_address: "http://192.168.1.100:50051".to_string(),
        http_address: "http://0.0.0.0:9999".to_string(),
        auth_token: "secret-token-xyz".to_string(),
        ..Config::default()
    };

    cfg.save_to_file(path.to_str().unwrap()).expect("save non-default");

    // 关键断言：文件存在且**包含**特定非默认字符串
    let content = std::fs::read_to_string(&path).expect("read back");
    assert!(
        content.contains("192.168.1.100"),
        "bridge_address must be written"
    );
    assert!(content.contains("9999"), "http_address must be written");
    assert!(
        content.contains("secret-token-xyz"),
        "auth_token must be written"
    );
}

#[test]
fn config_save_to_file_round_trip_with_jwt_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("jwt_default.toml");

    let original = Config::default();
    original.save_to_file(path.to_str().unwrap()).expect("save");

    // 关键断言：重新读取后 JWT 三个默认值不变
    // (mutation 抓"replace default_jwt_access_ttl_secs with 0/1/-1"
    //  → 因为 default_jwt_access_ttl_secs 已被 serde 写入文件,反序列化可还原真实值)
    let reloaded = Config::from_file(path.to_str().unwrap()).expect("from_file");
    assert_eq!(reloaded.jwt.access_ttl_secs, original.jwt.access_ttl_secs);
    assert_eq!(reloaded.jwt.refresh_ttl_secs, original.jwt.refresh_ttl_secs);
    assert_eq!(reloaded.jwt.issuer, original.jwt.issuer);
    // 进一步：验证默认值都是合理正值（防止替换为 0/负数）
    assert!(
        reloaded.jwt.access_ttl_secs > 0,
        "access_ttl_secs must be positive"
    );
    assert!(
        reloaded.jwt.refresh_ttl_secs > 0,
        "refresh_ttl_secs must be positive"
    );
    assert!(
        reloaded.jwt.refresh_ttl_secs > reloaded.jwt.access_ttl_secs,
        "refresh > access"
    );
    assert!(!reloaded.jwt.issuer.is_empty(), "issuer must be non-empty");
}

#[test]
fn config_default_jwt_ttl_values_are_sane() {
    // 显式验证 default_jwt_access_ttl_secs / refresh_ttl_secs / issuer 返回值
    // (mutation 抓"replace ... with 0/1/-1"或"with String::new()")
    let c = Config::default();
    assert!(c.jwt.access_ttl_secs > 0);
    assert!(c.jwt.refresh_ttl_secs > c.jwt.access_ttl_secs);
    assert!(!c.jwt.issuer.is_empty());
}
