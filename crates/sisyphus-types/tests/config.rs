//! Integration tests for sisyphus-types `config` module.
//!
//! Exercises the pure-data + validation surface of [`Config`]:
//! - Default values for top-level fields and the bridge/HTTP addresses
//! - `validate()` happy path and a few failure cases
//! - `save_to_file` / `from_file` round-trip via tempdir
//! - `apply_env_overrides` (verifying it doesn't panic on clean env)
//! - `resolve_env_placeholders_str` for known env vars

use sisyphus_types::config::Config;
use std::env;

// ---------------------------------------------------------------------------
// Default values
// ---------------------------------------------------------------------------

#[test]
fn config_default_bridge_address() {
    let c = Config::default();
    assert_eq!(c.bridge_address, "http://[::1]:50051");
}

#[test]
fn config_default_http_address() {
    let c = Config::default();
    assert_eq!(c.http_address, "http://[::1]:8080");
}

#[test]
fn config_default_auth_token_empty() {
    let c = Config::default();
    assert!(c.auth_token.is_empty());
}

#[test]
fn config_default_sub_configs_present() {
    let c = Config::default();
    // All sub-configs should be present (not panic).
    let _ = &c.llm;
    let _ = &c.cognitive_loop;
    let _ = &c.executor;
    let _ = &c.cache;
    let _ = &c.metrics;
    let _ = &c.sandbox;
    let _ = &c.mcp;
    let _ = &c.registry;
    let _ = &c.jcode;
    let _ = &c.chaos;
    let _ = &c.jwt;
}

// ---------------------------------------------------------------------------
// validate()
// ---------------------------------------------------------------------------

#[test]
fn config_validate_default_ok() {
    let c = Config::default();
    assert!(c.validate().is_ok());
}

#[test]
fn config_validate_auth_token_requires_jwt_secret() {
    // rustc 1.98 clippy::field_reassign_with_default 修复：struct update 语法
    let c = Config {
        auth_token: "my-token".to_string(),
        ..Config::default()
    };
    // jwt.secret is still empty → validation must fail.
    let r = c.validate();
    assert!(r.is_err());
    let s = format!("{}", r.unwrap_err());
    assert!(s.contains("jwt.secret"));
}

#[test]
fn config_validate_with_both_auth_token_and_jwt_secret_ok() {
    let c = Config {
        auth_token: "my-token".to_string(),
        jwt: sisyphus_types::JwtConfig {
            secret: "secret".to_string(),
            ..Default::default()
        },
        ..Config::default()
    };
    assert!(c.validate().is_ok());
}
// ---------------------------------------------------------------------------
// apply_env_overrides
// ---------------------------------------------------------------------------

#[test]
fn config_apply_env_overrides_does_not_panic() {
    let mut c = Config::default();
    c.apply_env_overrides();
    // Even with no env vars set, the call must complete.
    let _ = format!("{:?}", c);
}

// ---------------------------------------------------------------------------
// resolve_env_placeholders_str
// ---------------------------------------------------------------------------

#[test]
fn resolve_env_placeholders_substitutes() {
    // SAFETY: Single-threaded test for env::set_var.
    unsafe {
        env::set_var("SISYPHUS_TEST_ENV", "value123");
    }
    let result =
        sisyphus_types::config::resolve_env_placeholders_str("hello {env:SISYPHUS_TEST_ENV}!");
    assert!(result.contains("value123"));
    unsafe {
        env::remove_var("SISYPHUS_TEST_ENV");
    }
}

#[test]
fn resolve_env_placeholders_uses_default() {
    // Unset var + default → uses default.
    unsafe {
        env::remove_var("SISYPHUS_NOT_SET_VAR_XYZ");
    }
    let result = sisyphus_types::config::resolve_env_placeholders_str(
        "a={env:SISYPHUS_NOT_SET_VAR_XYZ|fallback}b",
    );
    assert!(result.contains("fallback"));
}

#[test]
fn resolve_env_placeholders_passthrough() {
    let input = "no placeholders here";
    let result = sisyphus_types::config::resolve_env_placeholders_str(input);
    assert_eq!(result, input);
}

// ---------------------------------------------------------------------------
// save_to_file / from_file round-trip
// ---------------------------------------------------------------------------

#[test]
fn config_save_and_load_roundtrip() {
    let dir = std::env::temp_dir().join(format!(
        "sisyphus-cfg-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");

    let original = Config::default();
    original.save_to_file(path.to_str().unwrap()).unwrap();
    let loaded = Config::from_file(path.to_str().unwrap()).unwrap();

    assert_eq!(loaded.bridge_address, original.bridge_address);
    assert_eq!(loaded.http_address, original.http_address);
    assert_eq!(loaded.auth_token, original.auth_token);

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}
