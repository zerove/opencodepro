//! Integration tests for sisyphus-tools-executor `executor::args` module's
//! pure argument-parsing helpers.

use serde_json::{json, Value};
use sisyphus_tools_executor::executor::args::{
    flexible_str_arg, flexible_str_arg_multi, ok_json, opt_str_arg, parse_args, path_arg, str_arg,
    u64_arg, usize_arg,
};

// ---------------------------------------------------------------------------
// parse_args
// ---------------------------------------------------------------------------

#[test]
fn parse_args_empty_bytes_returns_empty_object() {
    let v = parse_args(&[]).unwrap();
    assert_eq!(v, serde_json::json!({}));
}

#[test]
fn parse_args_valid_json() {
    let json = serde_json::to_vec(&json!({"k": "v"})).unwrap();
    let v = parse_args(&json).unwrap();
    assert_eq!(v["k"], "v");
}

#[test]
fn parse_args_invalid_json_fails() {
    let r = parse_args(b"not json");
    assert!(r.is_err());
}

#[test]
fn parse_args_array_form() {
    let json = serde_json::to_vec(&json!([1, 2, 3])).unwrap();
    let v = parse_args(&json).unwrap();
    assert!(v.is_array());
}

// ---------------------------------------------------------------------------
// str_arg
// ---------------------------------------------------------------------------

#[test]
fn str_arg_existing_string() {
    let v = json!({"name": "alice"});
    assert_eq!(str_arg(&v, "name").unwrap(), "alice");
}

#[test]
fn str_arg_missing_returns_error() {
    let v = json!({});
    assert!(str_arg(&v, "name").is_err());
}

#[test]
fn str_arg_wrong_type_returns_error() {
    let v = json!({"name": 42});
    assert!(str_arg(&v, "name").is_err());
}

#[test]
fn str_arg_empty_string_ok() {
    let v = json!({"name": ""});
    assert_eq!(str_arg(&v, "name").unwrap(), "");
}

// ---------------------------------------------------------------------------
// path_arg
// ---------------------------------------------------------------------------

#[test]
fn path_arg_existing() {
    let v = json!({"path": "/tmp/x.rs"});
    assert_eq!(path_arg(&v, "path").unwrap(), "/tmp/x.rs");
}

#[test]
fn path_arg_missing_error() {
    let v = json!({});
    assert!(path_arg(&v, "path").is_err());
}

// ---------------------------------------------------------------------------
// flexible_str_arg
// ---------------------------------------------------------------------------

#[test]
fn flexible_str_arg_primary() {
    let v = json!({"a": "1", "b": "2"});
    assert_eq!(flexible_str_arg(&v, "a", "b").unwrap(), "1");
}

#[test]
fn flexible_str_arg_fallback() {
    let v = json!({"b": "2"});
    assert_eq!(flexible_str_arg(&v, "a", "b").unwrap(), "2");
}

#[test]
fn flexible_str_arg_both_missing() {
    let v = json!({});
    assert!(flexible_str_arg(&v, "a", "b").is_err());
}

#[test]
fn flexible_str_arg_multi_first_match() {
    let v = json!({"a": "1"});
    let r = flexible_str_arg_multi(&v, &["a", "b", "c"]);
    assert!(r.is_ok());
    assert_eq!(r.unwrap(), "1");
}

#[test]
fn flexible_str_arg_multi_second_match() {
    let v = json!({"b": "2"});
    let r = flexible_str_arg_multi(&v, &["a", "b", "c"]);
    assert_eq!(r.unwrap(), "2");
}

#[test]
fn flexible_str_arg_multi_empty() {
    let v = json!({"a": "1"});
    let r = flexible_str_arg_multi(&v, &[]);
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------
// u64_arg
// ---------------------------------------------------------------------------

#[test]
fn u64_arg_existing() {
    let v = json!({"n": 42});
    assert_eq!(u64_arg(&v, "n").unwrap(), 42);
}

#[test]
fn u64_arg_missing_error() {
    let v = json!({});
    assert!(u64_arg(&v, "n").is_err());
}

#[test]
fn u64_arg_zero_ok() {
    let v = json!({"n": 0});
    assert_eq!(u64_arg(&v, "n").unwrap(), 0);
}

#[test]
fn u64_arg_large_value() {
    let v = json!({"n": u64::MAX});
    assert_eq!(u64_arg(&v, "n").unwrap(), u64::MAX);
}

// ---------------------------------------------------------------------------
// usize_arg
// ---------------------------------------------------------------------------

#[test]
fn usize_arg_existing() {
    let v = json!({"n": 100});
    assert_eq!(usize_arg(&v, "n").unwrap(), 100);
}

#[test]
fn usize_arg_missing_error() {
    let v = json!({});
    assert!(usize_arg(&v, "n").is_err());
}

// ---------------------------------------------------------------------------
// opt_str_arg
// ---------------------------------------------------------------------------

#[test]
fn opt_str_arg_present() {
    let v = json!({"x": "y"});
    assert_eq!(opt_str_arg(&v, "x"), Some("y".to_string()));
}

#[test]
fn opt_str_arg_absent() {
    let v = json!({});
    assert!(opt_str_arg(&v, "x").is_none());
}

#[test]
fn opt_str_arg_null() {
    let v = json!({"x": null});
    assert!(opt_str_arg(&v, "x").is_none());
}

#[test]
fn opt_str_arg_empty_string() {
    let v = json!({"x": ""});
    // empty string is still a valid string
    assert_eq!(opt_str_arg(&v, "x"), Some(String::new()));
}

// ---------------------------------------------------------------------------
// ok_json
// ---------------------------------------------------------------------------

#[test]
fn ok_json_returns_bytes() {
    let r = ok_json(json!({"k": 1})).unwrap();
    let back: Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(back["k"], 1);
}

#[test]
fn ok_json_null_value() {
    let r = ok_json(json!(null)).unwrap();
    let back: Value = serde_json::from_slice(&r).unwrap();
    assert!(back.is_null());
}

#[test]
fn ok_json_array() {
    let r = ok_json(json!([1, 2, 3])).unwrap();
    let back: Value = serde_json::from_slice(&r).unwrap();
    assert!(back.is_array());
}
