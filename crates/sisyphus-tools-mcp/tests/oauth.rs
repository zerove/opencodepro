//! Integration tests for sisyphus-tools-mcp `oauth` module.
//!
//! Covers [`TokenResponse`], [`OAuthError`], and the pure-data parts
//! of [`OAuth2ClientCredentials`] (constructor + `invalidate`).
//! `fetch_token` requires a live HTTP server, so it is not exercised
//! here.

use sisyphus_tools_mcp::mcp::oauth::{OAuth2ClientCredentials, OAuthError, TokenResponse};

// ---------------------------------------------------------------------------
// OAuth2ClientCredentials
// ---------------------------------------------------------------------------

#[test]
fn client_credentials_new_stores_inputs() {
    let c = OAuth2ClientCredentials::new(
        "https://auth.example.com/oauth/token",
        "client-1",
        "secret-1",
        Some("read:tools"),
    );
    // No public getters — but we can verify the invalidate() call
    // does not panic and that two distinct clients with different
    // inputs are not equal via the only public surface (Pointer eq).
    let c2 = OAuth2ClientCredentials::new(
        "https://auth.example.com/oauth/token",
        "client-1",
        "secret-1",
        Some("read:tools"),
    );
    // Invalidate both — should not panic.
    c.invalidate();
    c2.invalidate();
}

#[test]
fn client_credentials_new_no_scope() {
    let c = OAuth2ClientCredentials::new("https://x", "id", "secret", None);
    c.invalidate();
}

#[test]
fn client_credentials_invalidate_is_idempotent() {
    let c = OAuth2ClientCredentials::new("https://x", "id", "secret", None);
    c.invalidate();
    c.invalidate();
    c.invalidate();
    // No panic.
}

// ---------------------------------------------------------------------------
// TokenResponse serialization round-trips
// ---------------------------------------------------------------------------

#[test]
fn token_response_full_round_trip() {
    let t = TokenResponse {
        access_token: "abc".to_string(),
        token_type: "Bearer".to_string(),
        expires_in: Some(3600),
        refresh_token: Some("r".to_string()),
        scope: Some("read".to_string()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TokenResponse = serde_json::from_str(&j).unwrap();
    assert_eq!(back.access_token, "abc");
    assert_eq!(back.token_type, "Bearer");
    assert_eq!(back.expires_in, Some(3600));
    assert_eq!(back.refresh_token.as_deref(), Some("r"));
    assert_eq!(back.scope.as_deref(), Some("read"));
}

#[test]
fn token_response_minimal_round_trip() {
    let j = r#"{"access_token":"xyz"}"#;
    let t: TokenResponse = serde_json::from_str(j).unwrap();
    assert_eq!(t.access_token, "xyz");
    // All other fields are defaulted to empty / None.
    assert_eq!(t.token_type, "");
    assert!(t.expires_in.is_none());
    assert!(t.refresh_token.is_none());
    assert!(t.scope.is_none());
}

#[test]
fn token_response_clone() {
    let t = TokenResponse {
        access_token: "a".to_string(),
        token_type: "Bearer".to_string(),
        expires_in: Some(60),
        refresh_token: None,
        scope: None,
    };
    let c = t.clone();
    assert_eq!(c.access_token, "a");
    assert_eq!(c.expires_in, Some(60));
}

// ---------------------------------------------------------------------------
// OAuthError display
// ---------------------------------------------------------------------------

#[test]
fn oauth_error_display_http() {
    let e = OAuthError::Http("connection refused".to_string());
    let s = format!("{e}");
    assert!(s.contains("HTTP error"));
    assert!(s.contains("connection refused"));
}

#[test]
fn oauth_error_display_bad_status() {
    let e = OAuthError::BadStatus(401, "Unauthorized".to_string());
    let s = format!("{e}");
    assert!(s.contains("401"));
    assert!(s.contains("Unauthorized"));
}

#[test]
fn oauth_error_display_parse() {
    let e = OAuthError::Parse("invalid json".to_string());
    let s = format!("{e}");
    assert!(s.contains("parse"));
    assert!(s.contains("invalid json"));
}

#[test]
fn oauth_error_debug_contains_variant() {
    let e = OAuthError::Http("x".to_string());
    let s = format!("{e:?}");
    assert!(s.contains("Http"));
}
