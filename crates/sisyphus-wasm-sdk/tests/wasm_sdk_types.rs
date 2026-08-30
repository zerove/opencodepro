//! 集成测试：wasm-sdk 简单数据类型 + 注册 API
//!
//! 覆盖 `sisyphus_wasm_sdk`：
//! - `ToolContext`/`ToolOutput` 字段
//! - `ToolOutput::json` 构造

use sisyphus_wasm_sdk::{ToolContext, ToolOutput};

#[test]
fn context_field_access() {
    let c = ToolContext {
        tool_id: "t1".to_string(),
        input: serde_json::json!({"x": 1}),
        state: serde_json::json!({"s": "running"}),
    };
    assert_eq!(c.tool_id, "t1");
    assert_eq!(c.input, serde_json::json!({"x": 1}));
    assert_eq!(c.state, serde_json::json!({"s": "running"}));
}

#[test]
fn context_null_state() {
    let c = ToolContext {
        tool_id: "t".to_string(),
        input: serde_json::json!(null),
        state: serde_json::json!(null),
    };
    assert_eq!(c.input, serde_json::json!(null));
    assert_eq!(c.state, serde_json::json!(null));
}

#[test]
fn context_clone() {
    let c = ToolContext {
        tool_id: "t".to_string(),
        input: serde_json::json!(42),
        state: serde_json::json!({}),
    };
    let c2 = c.clone();
    assert_eq!(c.tool_id, c2.tool_id);
    assert_eq!(c.input, c2.input);
    assert_eq!(c.state, c2.state);
}

#[test]
fn context_debug() {
    let c = ToolContext {
        tool_id: "my_tool".to_string(),
        input: serde_json::json!("in"),
        state: serde_json::json!("st"),
    };
    let s = format!("{c:?}");
    assert!(s.contains("ToolContext"));
    assert!(s.contains("my_tool"));
}

#[test]
fn context_serde_round_trip() {
    let c = ToolContext {
        tool_id: "t1".to_string(),
        input: serde_json::json!({"x": 1, "y": [1, 2]}),
        state: serde_json::json!({"counter": 5}),
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: ToolContext = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tool_id, "t1");
    assert_eq!(back.input, c.input);
    assert_eq!(back.state, c.state);
}

#[test]
fn output_json_value() {
    let v = serde_json::json!({"key": "value", "n": 42});
    let o = ToolOutput::json(v.clone());
    assert_eq!(o.result, v);
    assert!(o.state_update.is_none());
}

#[test]
fn output_json_null() {
    let o = ToolOutput::json(serde_json::json!(null));
    assert_eq!(o.result, serde_json::json!(null));
    assert!(o.state_update.is_none());
}

#[test]
fn output_clone() {
    let o = ToolOutput::json(serde_json::json!(1));
    let o2 = o.clone();
    assert_eq!(o.result, o2.result);
    assert_eq!(o.state_update, o2.state_update);
}

#[test]
fn output_debug() {
    let o = ToolOutput::json(serde_json::json!("hello"));
    let s = format!("{o:?}");
    assert!(s.contains("ToolOutput"));
    assert!(s.contains("hello"));
}

#[test]
fn output_field_construction() {
    let o = ToolOutput {
        result: serde_json::json!("x"),
        state_update: Some(serde_json::json!({"updated": true})),
    };
    assert_eq!(o.result, serde_json::json!("x"));
    assert_eq!(o.state_update, Some(serde_json::json!({"updated": true})));
}
