//! Integration tests for sisyphus-types `NodeOutput` (execution.rs).
//!
//! Exercises the public `NodeOutput` type and its `into_inner` /
//! `as_value` / `explicit_status` / `indicates_failure` methods.
//! `NodeOutput` is the universal wrapper around `serde_json::Value`
//! that flows through every node in the SOP graph.

use serde_json::{json, Value};
use sisyphus_types::NodeOutput;

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn node_output_from_value() {
    let n = NodeOutput::from(json!("hello"));
    assert_eq!(n.as_value(), &json!("hello"));
}

#[test]
fn node_output_from_complex_value() {
    let n = NodeOutput::from(json!({"status": "ok", "code": 200}));
    let v = n.as_value();
    assert_eq!(v["status"], "ok");
    assert_eq!(v["code"], 200);
}

#[test]
fn node_output_default_is_null() {
    let n = NodeOutput::default();
    assert_eq!(n.as_value(), &Value::Null);
}

#[test]
fn node_output_into_inner_consumes_wrapper() {
    let n = NodeOutput::from(json!(42));
    let v = n.into_inner();
    assert_eq!(v, json!(42));
}

// ---------------------------------------------------------------------------
// explicit_status
// ---------------------------------------------------------------------------

#[test]
fn explicit_status_no_signal_returns_none() {
    let n = NodeOutput::from(json!({"unrelated": "field"}));
    assert!(n.explicit_status().is_none());
}

#[test]
fn explicit_status_plain_string_value_returns_none() {
    // A string value with no `status` key inside an object.
    let n = NodeOutput::from(json!("just a string"));
    assert!(n.explicit_status().is_none());
}

#[test]
fn explicit_status_object_with_status_error() {
    let n = NodeOutput::from(json!({"status": "error", "msg": "boom"}));
    assert_eq!(n.explicit_status(), Some(false));
}

#[test]
fn explicit_status_object_with_status_failed() {
    let n = NodeOutput::from(json!({"status": "failed"}));
    assert_eq!(n.explicit_status(), Some(false));
}

#[test]
fn explicit_status_object_with_status_ok() {
    let n = NodeOutput::from(json!({"status": "ok"}));
    assert_eq!(n.explicit_status(), Some(true));
}

#[test]
fn explicit_status_object_with_status_success() {
    let n = NodeOutput::from(json!({"status": "success"}));
    assert_eq!(n.explicit_status(), Some(true));
}

#[test]
fn explicit_status_object_with_status_completed() {
    let n = NodeOutput::from(json!({"status": "completed"}));
    assert_eq!(n.explicit_status(), Some(true));
}

#[test]
fn explicit_status_status_field_is_case_insensitive() {
    let n = NodeOutput::from(json!({"status": "ERROR"}));
    assert_eq!(n.explicit_status(), Some(false));
    let n = NodeOutput::from(json!({"status": "Failed"}));
    assert_eq!(n.explicit_status(), Some(false));
}

#[test]
fn explicit_status_with_success_true() {
    let n = NodeOutput::from(json!({"success": true}));
    assert_eq!(n.explicit_status(), Some(true));
}

#[test]
fn explicit_status_with_success_false() {
    let n = NodeOutput::from(json!({"success": false}));
    assert_eq!(n.explicit_status(), Some(false));
}

#[test]
fn explicit_status_with_exit_code_zero() {
    let n = NodeOutput::from(json!({"exit_code": 0}));
    assert_eq!(n.explicit_status(), Some(true));
}

#[test]
fn explicit_status_with_exit_code_nonzero() {
    let n = NodeOutput::from(json!({"exit_code": 1}));
    assert_eq!(n.explicit_status(), Some(false));
    let n = NodeOutput::from(json!({"exit_code": 127}));
    assert_eq!(n.explicit_status(), Some(false));
}

#[test]
fn explicit_status_priority_status_over_success() {
    // When `status` is present, it wins over `success`.
    let n = NodeOutput::from(json!({"status": "error", "success": true}));
    assert_eq!(n.explicit_status(), Some(false));
}

#[test]
fn explicit_status_priority_success_over_exit_code() {
    let n = NodeOutput::from(json!({"success": false, "exit_code": 0}));
    assert_eq!(n.explicit_status(), Some(false));
}

// ---------------------------------------------------------------------------
// indicates_failure
// ---------------------------------------------------------------------------

#[test]
fn indicates_failure_with_status_error() {
    let n = NodeOutput::from(json!({"status": "error"}));
    assert!(n.indicates_failure());
}

#[test]
fn indicates_failure_with_status_ok_is_false() {
    let n = NodeOutput::from(json!({"status": "ok"}));
    assert!(!n.indicates_failure());
}

#[test]
fn indicates_failure_with_success_false() {
    let n = NodeOutput::from(json!({"success": false}));
    assert!(n.indicates_failure());
}

#[test]
fn indicates_failure_with_success_true_is_false() {
    let n = NodeOutput::from(json!({"success": true}));
    assert!(!n.indicates_failure());
}

#[test]
fn indicates_failure_text_sniffing_for_plain_strings() {
    // No explicit status; fallback to text sniffing.
    assert!(NodeOutput::from(json!("some error occurred")).indicates_failure());
    assert!(NodeOutput::from(json!("task failed")).indicates_failure());
    assert!(!NodeOutput::from(json!("task succeeded")).indicates_failure());
}

#[test]
fn indicates_failure_text_sniffing_is_case_insensitive() {
    assert!(NodeOutput::from(json!("ERROR happened")).indicates_failure());
    assert!(NodeOutput::from(json!("Failed to do X")).indicates_failure());
}

#[test]
fn indicates_failure_empty_output_is_false() {
    let n = NodeOutput::from(json!(""));
    assert!(!n.indicates_failure());
}

#[test]
fn indicates_failure_null_is_false() {
    let n = NodeOutput::default();
    assert!(!n.indicates_failure());
}

#[test]
fn indicates_failure_object_with_unrelated_fields_falls_back() {
    // No status/success/exit_code → text-sniff the rendered JSON.
    let n = NodeOutput::from(json!({"data": "everything is fine"}));
    assert!(!n.indicates_failure());

    let n = NodeOutput::from(json!({"data": "an error happened"}));
    assert!(n.indicates_failure());
}

// ---------------------------------------------------------------------------
// Display / Debug / PartialEq
// ---------------------------------------------------------------------------

#[test]
fn node_output_display_renders_inner_value() {
    let n = NodeOutput::from(json!("hello"));
    // Display formats the value as JSON (strings are quoted).
    assert_eq!(format!("{n}"), "\"hello\"");
}

#[test]
fn node_output_partial_eq_with_value() {
    let n = NodeOutput::from(json!(42));
    assert!(n == Value::from(42));
    assert!(n != Value::from(43));
}

#[test]
fn node_output_partial_eq_with_itself() {
    let a = NodeOutput::from(json!({"a": 1}));
    let b = NodeOutput::from(json!({"a": 1}));
    let c = NodeOutput::from(json!({"a": 2}));
    assert_eq!(a, b);
    assert_ne!(a, c);
}
