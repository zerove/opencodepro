//! Integration tests for sisyphus-core `internalization_manager`.
//!
//! Exercises the public `InternalizationRequest` data type used
//! to feed human corrections into the internalization pipeline.
//! The pipeline itself is LLM-driven, so the type-level test
//! covers construction, clone, and field access.

use sisyphus_core::internalization_manager::InternalizationRequest;

#[test]
fn internalization_request_construction() {
    let r = InternalizationRequest {
        node_id: "node_a".into(),
        prompt: "refactor the foo function".into(),
        context: "current implementation has O(n²) complexity".into(),
        correction: "use a HashMap to reduce to O(n)".into(),
    };
    assert_eq!(r.node_id, "node_a");
    assert_eq!(r.prompt, "refactor the foo function");
    assert_eq!(r.context, "current implementation has O(n²) complexity");
    assert_eq!(r.correction, "use a HashMap to reduce to O(n)");
}

#[test]
fn internalization_request_clone() {
    let r = InternalizationRequest {
        node_id: "n".into(),
        prompt: "p".into(),
        context: "c".into(),
        correction: "x".into(),
    };
    let cloned = r.clone();
    assert_eq!(cloned.node_id, "n");
    assert_eq!(cloned.prompt, "p");
    assert_eq!(cloned.context, "c");
    assert_eq!(cloned.correction, "x");
}

#[test]
fn internalization_request_with_empty_strings() {
    let r = InternalizationRequest {
        node_id: "".into(),
        prompt: "".into(),
        context: "".into(),
        correction: "".into(),
    };
    let cloned = r.clone();
    assert!(cloned.node_id.is_empty());
    assert!(cloned.prompt.is_empty());
}

#[test]
fn internalization_request_with_unicode() {
    let r = InternalizationRequest {
        node_id: "节点".into(),
        prompt: "重构函数".into(),
        context: "当前是 O(n²)".into(),
        correction: "用 HashMap 降到 O(n)".into(),
    };
    assert_eq!(r.node_id, "节点");
    assert!(r.correction.contains("HashMap"));
}

#[test]
fn internalization_request_with_long_text() {
    let long_text = "a".repeat(10_000);
    let r = InternalizationRequest {
        node_id: "n".into(),
        prompt: long_text.clone(),
        context: long_text.clone(),
        correction: long_text,
    };
    assert_eq!(r.prompt.len(), 10_000);
}
