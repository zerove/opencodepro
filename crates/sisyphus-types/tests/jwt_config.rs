//! Integration tests for sisyphus-types `config::JwtConfig`.
//!
//! Exercises the public surface of [`JwtConfig`]: default values,
//! field assignment, and serde round-trip.

use sisyphus_types::config::JwtConfig;

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

#[test]
fn jwt_config_default_secret_is_empty() {
    let c = JwtConfig::default();
    // The default for `secret` is empty — must be configured via env or
    // config file in production.
    assert!(c.secret.is_empty());
}

#[test]
fn jwt_config_default_access_ttl_is_15_minutes() {
    let c = JwtConfig::default();
    assert_eq!(c.access_ttl_secs, 900);
}

#[test]
fn jwt_config_default_refresh_ttl_is_7_days() {
    let c = JwtConfig::default();
    assert_eq!(c.refresh_ttl_secs, 604800);
}

#[test]
fn jwt_config_default_issuer() {
    let c = JwtConfig::default();
    assert_eq!(c.issuer, "sisyphus-bridge");
}

// ---------------------------------------------------------------------------
// Field assignment
// ---------------------------------------------------------------------------

#[test]
fn jwt_config_field_assignment() {
    let c = JwtConfig {
        secret: "my-secret".into(),
        access_ttl_secs: 3600,
        refresh_ttl_secs: 86_400,
        issuer: "my-issuer".into(),
    };
    assert_eq!(c.secret, "my-secret");
    assert_eq!(c.access_ttl_secs, 3600);
    assert_eq!(c.refresh_ttl_secs, 86_400);
    assert_eq!(c.issuer, "my-issuer");
}

#[test]
fn jwt_config_field_assignment_negative_ttl() {
    // Negative TTLs are not validated at the type level; verify the
    // value round-trips.
    let c = JwtConfig {
        secret: "s".into(),
        access_ttl_secs: -1,
        refresh_ttl_secs: -2,
        issuer: "i".into(),
    };
    assert_eq!(c.access_ttl_secs, -1);
    assert_eq!(c.refresh_ttl_secs, -2);
}

// ---------------------------------------------------------------------------
// Serde
// ---------------------------------------------------------------------------

#[test]
fn jwt_config_serde_full() {
    let c = JwtConfig {
        secret: "x".into(),
        access_ttl_secs: 60,
        refresh_ttl_secs: 120,
        issuer: "issuer".into(),
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: JwtConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.secret, "x");
    assert_eq!(back.access_ttl_secs, 60);
    assert_eq!(back.refresh_ttl_secs, 120);
    assert_eq!(back.issuer, "issuer");
}

#[test]
fn jwt_config_serde_minimal() {
    // Only `secret` field provided; the rest use `serde(default = ...)`.
    let j = r#"{"secret": "s"}"#;
    let c: JwtConfig = serde_json::from_str(j).unwrap();
    assert_eq!(c.secret, "s");
    // Defaults should be applied.
    assert_eq!(c.access_ttl_secs, 900);
    assert_eq!(c.refresh_ttl_secs, 604800);
    assert_eq!(c.issuer, "sisyphus-bridge");
}

#[test]
fn jwt_config_serde_empty() {
    // All fields use defaults.
    let j = "{}";
    let c: JwtConfig = serde_json::from_str(j).unwrap();
    assert!(c.secret.is_empty());
    assert_eq!(c.access_ttl_secs, 900);
}
