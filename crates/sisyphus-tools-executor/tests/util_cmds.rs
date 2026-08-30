//! Integration tests for sisyphus-tools-executor `executor::util_cmds` module's
//! pure command implementations: json_query, base64_convert, url_convert, time_convert,
//! path_expand.

use serde_json::{json, Value};
use sisyphus_tools_executor::executor::util_cmds::{
    cmd_base64_convert, cmd_json_query, cmd_path_expand, cmd_time_convert, cmd_url_convert,
};

// ---------------------------------------------------------------------------
// cmd_json_query
// ---------------------------------------------------------------------------

#[test]
fn json_query_top_level_field() {
    let v = cmd_json_query(json!({
        "json": r#"{"name": "alice", "age": 30}"#,
        "path": "name"
    }))
    .unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    // cmd_json_query returns the value directly, not wrapped in {"result":...}
    assert_eq!(back, "alice");
}

#[test]
fn json_query_nested_field() {
    let v = cmd_json_query(json!({
        "json": r#"{"user": {"profile": {"email": "a@b.com"}}}"#,
        "path": "user.profile.email"
    }))
    .unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert_eq!(back, "a@b.com");
}

#[test]
fn json_query_numeric() {
    let v = cmd_json_query(json!({
        "json": r#"{"count": 42}"#,
        "path": "count"
    }))
    .unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert_eq!(back, 42);
}

#[test]
fn json_query_array_index() {
    let v = cmd_json_query(json!({
        "json": r#"{"items": [10, 20, 30]}"#,
        "path": "items[1]"
    }))
    .unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert_eq!(back, 20);
}

#[test]
fn json_query_missing_path_arg() {
    let r = cmd_json_query(json!({"json": "{}"}));
    assert!(r.is_err());
}

#[test]
fn json_query_missing_json_arg() {
    let r = cmd_json_query(json!({"path": "x"}));
    assert!(r.is_err());
}

#[test]
fn json_query_invalid_json() {
    let r = cmd_json_query(json!({"json": "not json", "path": "x"}));
    assert!(r.is_err());
}

#[test]
fn json_query_path_not_found() {
    let r = cmd_json_query(json!({
        "json": r#"{"a": 1}"#,
        "path": "missing"
    }));
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------
// cmd_base64_convert
// ---------------------------------------------------------------------------

#[test]
fn base64_encode_default() {
    let v = cmd_base64_convert(json!({"input": "hello"})).unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    // base64 of "hello" is "aGVsbG8="
    assert_eq!(back["result"], "aGVsbG8=");
}

#[test]
fn base64_encode_explicit() {
    let v = cmd_base64_convert(json!({"input": "world", "mode": "encode"})).unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert_eq!(back["result"], "d29ybGQ=");
}

#[test]
fn base64_decode_round_trip() {
    let v = cmd_base64_convert(json!({"input": "aGVsbG8=", "mode": "decode"})).unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert_eq!(back["result"], "hello");
}

#[test]
fn base64_missing_input() {
    let r = cmd_base64_convert(json!({}));
    assert!(r.is_err());
}

#[test]
fn base64_decode_invalid() {
    let r = cmd_base64_convert(json!({"input": "!!!not base64!!!", "mode": "decode"}));
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------
// cmd_url_convert
// ---------------------------------------------------------------------------

#[test]
fn url_encode_default() {
    let v = cmd_url_convert(json!({"input": "hello world"})).unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert_eq!(back["result"], "hello%20world");
}

#[test]
fn url_encode_explicit() {
    let v = cmd_url_convert(json!({"input": "a&b=c", "mode": "encode"})).unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert_eq!(back["result"], "a%26b%3Dc");
}

#[test]
fn url_decode_round_trip() {
    let v = cmd_url_convert(json!({"input": "hello%20world", "mode": "decode"})).unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert_eq!(back["result"], "hello world");
}

#[test]
fn url_missing_input() {
    let r = cmd_url_convert(json!({}));
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------
// cmd_path_expand
// ---------------------------------------------------------------------------

#[test]
fn path_expand_absolute_unchanged() {
    let v = cmd_path_expand(json!({"path": "/abs/path"})).unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert_eq!(back["expanded_path"], "/abs/path");
}

#[test]
fn path_expand_relative_unchanged() {
    let v = cmd_path_expand(json!({"path": "rel/path"})).unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert_eq!(back["expanded_path"], "rel/path");
}

#[test]
fn path_expand_tilde_with_home() {
    // When HOME is set, ~ should expand
    let v = cmd_path_expand(json!({"path": "~/x"})).unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    let expanded = back["expanded_path"].as_str().unwrap();
    // Should not contain ~
    assert!(!expanded.contains('~'));
}

#[test]
fn path_expand_missing_path() {
    let r = cmd_path_expand(json!({}));
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------
// cmd_time_convert
// ---------------------------------------------------------------------------

#[test]
fn time_convert_utc_default() {
    let v = cmd_time_convert(json!({
        "time": "2024-01-15T12:00:00Z"
    }))
    .unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert!(back["converted_time"].is_string());
}

#[test]
fn time_convert_utc_explicit() {
    let v = cmd_time_convert(json!({
        "time": "2024-01-15T12:00:00Z",
        "timezone": "UTC"
    }))
    .unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert!(back["converted_time"].is_string());
}

#[test]
fn time_convert_local() {
    let v = cmd_time_convert(json!({
        "time": "2024-01-15T12:00:00Z",
        "timezone": "Local"
    }))
    .unwrap();
    let back: Value = serde_json::from_slice(&v).unwrap();
    assert!(back["converted_time"].is_string());
}

#[test]
fn time_convert_unsupported_timezone() {
    let r = cmd_time_convert(json!({
        "time": "2024-01-15T12:00:00Z",
        "timezone": "Mars/Olympus"
    }));
    assert!(r.is_err());
}

#[test]
fn time_convert_invalid_iso_format() {
    let r = cmd_time_convert(json!({
        "time": "not a time"
    }));
    assert!(r.is_err());
}

#[test]
fn time_convert_missing_time() {
    let r = cmd_time_convert(json!({}));
    assert!(r.is_err());
}
