//! 集成测试：core InternalizationRequest 简单数据
//!
//! 覆盖 `sisyphus_core::internalization_manager::InternalizationRequest`：
//! - 字段访问
//! - Clone / Debug

use sisyphus_core::internalization_manager::InternalizationRequest;

#[test]
fn request_field_access() {
    let r = InternalizationRequest {
        node_id: "n1".to_string(),
        prompt: "build a thing".to_string(),
        context: "ctx".to_string(),
        correction: "use Rust instead".to_string(),
    };
    assert_eq!(r.node_id, "n1");
    assert_eq!(r.prompt, "build a thing");
    assert_eq!(r.context, "ctx");
    assert_eq!(r.correction, "use Rust instead");
}

#[test]
fn request_clone() {
    let r = InternalizationRequest {
        node_id: "n".to_string(),
        prompt: "p".to_string(),
        context: "c".to_string(),
        correction: "k".to_string(),
    };
    let r2 = r.clone();
    assert_eq!(r.node_id, r2.node_id);
    assert_eq!(r.prompt, r2.prompt);
    assert_eq!(r.context, r2.context);
    assert_eq!(r.correction, r2.correction);
}

#[test]
fn request_debug() {
    let r = InternalizationRequest {
        node_id: "n".to_string(),
        prompt: "p".to_string(),
        context: "c".to_string(),
        correction: "k".to_string(),
    };
    let s = format!("{r:?}");
    assert!(s.contains("InternalizationRequest"));
    assert!(s.contains("n"));
    assert!(s.contains("p"));
}

#[test]
fn request_empty_strings() {
    let r = InternalizationRequest {
        node_id: String::new(),
        prompt: String::new(),
        context: String::new(),
        correction: String::new(),
    };
    assert_eq!(r.node_id, "");
    assert_eq!(r.prompt, "");
}

#[test]
fn request_long_strings() {
    let long = "a".repeat(10_000);
    let r = InternalizationRequest {
        node_id: long.clone(),
        prompt: long.clone(),
        context: long.clone(),
        correction: long,
    };
    assert_eq!(r.node_id.len(), 10_000);
}

#[test]
fn request_unicode() {
    let r = InternalizationRequest {
        node_id: "节点_1".to_string(),
        prompt: "构建特性".to_string(),
        context: "上下文".to_string(),
        correction: "修正".to_string(),
    };
    assert_eq!(r.node_id, "节点_1");
    assert_eq!(r.prompt, "构建特性");
}

#[test]
fn request_equality_after_clone() {
    let r = InternalizationRequest {
        node_id: "n".to_string(),
        prompt: "p".to_string(),
        context: "c".to_string(),
        correction: "k".to_string(),
    };
    let r2 = r.clone();
    let r3 = r.clone();
    // Field-by-field equality via clone
    assert_eq!(r2.node_id, r3.node_id);
    assert_eq!(r2.prompt, r3.prompt);
}

#[test]
fn request_construct_with_move() {
    // Strings can be moved in
    let node_id = String::from("n1");
    let prompt = String::from("p");
    let context = String::from("c");
    let correction = String::from("k");
    let r = InternalizationRequest {
        node_id,
        prompt,
        context,
        correction,
    };
    assert_eq!(r.node_id, "n1");
}
