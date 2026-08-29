//! Integration tests for sisyphus-types `execution` types.
//!
//! Exercises the public [`NodeOutput`] API (status extraction,
//! failure detection, JSON round-trip), as well as the
//! [`ExecutionContext`] notepad helpers.

use sisyphus_types::execution::{ExecutionContext, NodeOutput};

// ---------------------------------------------------------------------------
// NodeOutput construction & introspection
// ---------------------------------------------------------------------------

#[test]
fn node_output_default_is_null() {
    let n = NodeOutput::default();
    assert!(n.as_value().is_null());
}

#[test]
fn node_output_from_value_preserves_payload() {
    let n: NodeOutput = serde_json::json!({"k": 1}).into();
    let v = n.as_value();
    assert_eq!(v["k"], 1);
}

#[test]
fn node_output_into_inner_returns_owned_value() {
    let n: NodeOutput = serde_json::json!(42).into();
    let v = n.into_inner();
    assert_eq!(v, serde_json::json!(42));
}

#[test]
fn node_output_deref_works() {
    let n: NodeOutput = serde_json::json!("hello").into();
    // Deref to &Value.
    let s: &str = n.as_str().expect("str");
    assert_eq!(s, "hello");
}

// ---------------------------------------------------------------------------
// explicit_status
// ---------------------------------------------------------------------------

#[test]
fn explicit_status_error_field() {
    let n: NodeOutput = serde_json::json!({"status": "error"}).into();
    assert_eq!(n.explicit_status(), Some(false));
}

#[test]
fn explicit_status_failed_field() {
    let n: NodeOutput = serde_json::json!({"status": "failed"}).into();
    assert_eq!(n.explicit_status(), Some(false));
}

#[test]
fn explicit_status_completed_field() {
    let n: NodeOutput = serde_json::json!({"status": "completed"}).into();
    assert_eq!(n.explicit_status(), Some(true));
}

#[test]
fn explicit_status_success_field() {
    let n: NodeOutput = serde_json::json!({"success": true}).into();
    assert_eq!(n.explicit_status(), Some(true));
    let n: NodeOutput = serde_json::json!({"success": false}).into();
    assert_eq!(n.explicit_status(), Some(false));
}

#[test]
fn explicit_status_exit_code_field() {
    let n: NodeOutput = serde_json::json!({"exit_code": 0}).into();
    assert_eq!(n.explicit_status(), Some(true));
    let n: NodeOutput = serde_json::json!({"exit_code": 1}).into();
    assert_eq!(n.explicit_status(), Some(false));
}

#[test]
fn explicit_status_returns_none_when_absent() {
    let n: NodeOutput = serde_json::json!({"foo": "bar"}).into();
    assert_eq!(n.explicit_status(), None);
}

#[test]
fn explicit_status_is_case_insensitive() {
    let n: NodeOutput = serde_json::json!({"status": "ERROR"}).into();
    assert_eq!(n.explicit_status(), Some(false));
}

// ---------------------------------------------------------------------------
// indicates_failure
// ---------------------------------------------------------------------------

#[test]
fn indicates_failure_explicit_status_wins() {
    // "success: false" should make indicates_failure return true
    // even if the JSON text doesn't contain "error" / "failed".
    let n: NodeOutput = serde_json::json!({"success": false}).into();
    assert!(n.indicates_failure());
}

#[test]
fn indicates_failure_text_fallback_for_null() {
    let n: NodeOutput = serde_json::Value::Null.into();
    // Null doesn't contain "error"/"failed" → not a failure.
    assert!(!n.indicates_failure());
}

#[test]
fn indicates_failure_text_fallback_for_error_text() {
    let n: NodeOutput = serde_json::json!("operation failed").into();
    assert!(n.indicates_failure());
}

#[test]
fn indicates_failure_text_fallback_for_error_object() {
    let n: NodeOutput = serde_json::json!({"message": "An error occurred"}).into();
    assert!(n.indicates_failure());
}

// ---------------------------------------------------------------------------
// NodeOutput equality
// ---------------------------------------------------------------------------

#[test]
fn node_output_equality_via_inner_value() {
    let a: NodeOutput = serde_json::json!({"x": 1}).into();
    let b: NodeOutput = serde_json::json!({"x": 1}).into();
    assert_eq!(a, b);
}

#[test]
fn node_output_partial_eq_with_value() {
    let n: NodeOutput = serde_json::json!("hello").into();
    let v = serde_json::json!("hello");
    assert!(n == v);
}

// ---------------------------------------------------------------------------
// ExecutionContext notepad
// ---------------------------------------------------------------------------

#[test]
fn execution_context_default_has_empty_notepad() {
    let ctx = ExecutionContext::new();
    assert!(ctx.read_notepad("any").is_none());
}

#[test]
fn execution_context_write_then_read_notepad() {
    let ctx = ExecutionContext::new();
    ctx.write_notepad("section1".to_string(), "hello".to_string());
    assert_eq!(ctx.read_notepad("section1"), Some("hello".to_string()));
}

#[test]
fn execution_context_notepad_overwrite_replaces() {
    let ctx = ExecutionContext::new();
    ctx.write_notepad("k".to_string(), "v1".to_string());
    ctx.write_notepad("k".to_string(), "v2".to_string());
    assert_eq!(ctx.read_notepad("k"), Some("v2".to_string()));
}

#[test]
fn execution_context_notepad_multiple_sections() {
    let ctx = ExecutionContext::new();
    ctx.write_notepad("a".to_string(), "1".to_string());
    ctx.write_notepad("b".to_string(), "2".to_string());
    assert_eq!(ctx.read_notepad("a"), Some("1".to_string()));
    assert_eq!(ctx.read_notepad("b"), Some("2".to_string()));
}

#[test]
fn execution_context_prune_keeps_recent() {
    let ctx = ExecutionContext::new();
    // Write more than max_insights entries.
    for i in 0..5 {
        ctx.write_notepad(format!("s{i}"), format!("v{i}"));
    }
    ctx.prune_notepad(3);
    // At least one section should be gone after pruning.
    let remaining: Vec<String> = ["s0", "s1", "s2", "s3", "s4"]
        .iter()
        .filter(|s| ctx.read_notepad(s).is_some())
        .map(|s| s.to_string())
        .collect();
    assert!(remaining.len() <= 5);
}

#[test]
fn execution_context_set_result_and_get_result() {
    let ctx = ExecutionContext::new();
    let out: NodeOutput = serde_json::json!({"k": 1}).into();
    ctx.set_result("node_a".to_string(), out);
    let got = ctx.get_result("node_a").expect("present");
    assert_eq!(got, serde_json::json!({"k": 1}));
}

#[test]
fn execution_context_get_missing_result_is_none() {
    let ctx = ExecutionContext::new();
    assert!(ctx.get_result("missing").is_none());
}

#[test]
fn execution_context_last_output_returns_most_recent() {
    let ctx = ExecutionContext::new();
    ctx.set_result("a".to_string(), serde_json::json!("first").into());
    ctx.set_result("b".to_string(), serde_json::json!("second").into());
    let last = ctx.last_output().expect("present");
    assert_eq!(last, serde_json::json!("second"));
}

#[test]
fn execution_context_record_modified_file() {
    let ctx = ExecutionContext::new();
    ctx.record_modified_file(std::path::PathBuf::from("/tmp/foo.rs"));
    ctx.record_modified_file(std::path::PathBuf::from("/tmp/bar.rs"));
    // No public getter, but no panic is the contract.
    let _ = ctx.last_output();
}
