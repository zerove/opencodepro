//! Integration tests for sisyphus-types Config::validate rules.
//!
//! Exercises the validation invariants so we catch regressions when new
//! fields are added. The validate() method must surface a
//! [`TypesError::ConfigValidationError`] for each currently-enforced rule.

use sisyphus_types::{Config, TypesError};

fn err_msg(r: Result<(), TypesError>) -> String {
    match r {
        Ok(()) => panic!("expected Err, got Ok"),
        Err(e) => format!("{e:?}"),
    }
}

#[test]
fn minimal_config_validates_ok() {
    // Empty/default config is valid (no auth, default bridge address).
    let cfg = Config::default();
    cfg.validate().expect("default config should be valid");
}

#[test]
fn auth_token_without_jwt_secret_is_rejected() {
    let mut cfg = Config::default();
    cfg.auth_token = "my-token".to_string();
    // jwt.secret left empty
    let msg = err_msg(cfg.validate());
    assert!(
        msg.contains("jwt.secret") || msg.contains("JWT"),
        "expected JWT error, got {msg}"
    );
}

#[test]
fn auth_token_with_jwt_secret_validates() {
    let mut cfg = Config::default();
    cfg.auth_token = "my-token".to_string();
    cfg.jwt.secret = "s3cr3t".to_string();
    cfg.validate().expect("auth+secret should be valid");
}

#[test]
fn invalid_bridge_address_scheme_is_rejected() {
    let mut cfg = Config::default();
    cfg.bridge_address = "ftp://server:21".to_string();
    let msg = err_msg(cfg.validate());
    assert!(
        msg.contains("bridge_address") || msg.contains("http"),
        "expected bridge_address error, got {msg}"
    );
}

#[test]
fn empty_bridge_address_is_rejected() {
    let mut cfg = Config::default();
    cfg.bridge_address = String::new();
    let msg = err_msg(cfg.validate());
    assert!(
        msg.contains("bridge_address"),
        "expected empty-address error, got {msg}"
    );
}
