//! Integration tests for sisyphus-wasm-sdk public API.
//!
//! Exercises the parts of the public surface that are safe to use from
//! `tests/`: `ToolOutput::json` constructor, the `SisyphusTool` trait
//! contract via a small test tool, and the in-process registry
//! `register_tool` / `get_tool` / `list_tools` / `tool_count` /
//! `remove_tool` round-trip.
//!
//! Note: the registry is process-global (a `OnceLock<Mutex<HashMap>>`),
//! so each test uses a unique tool id to avoid interference when the
//! whole binary is run in parallel.

use async_trait::async_trait;
use serde_json::{json, Value};
use sisyphus_wasm_sdk::{
    get_tool, list_tools, register_tool_as, remove_tool, tool_count, SisyphusTool, ToolContext,
    ToolOutput,
};
use std::sync::atomic::{AtomicU64, Ordering};

/// Atomic counter shared by all instances so tests can verify call counts.
static CALL_COUNT: AtomicU64 = AtomicU64::new(0);

struct TestAddTool;

#[async_trait]
impl SisyphusTool for TestAddTool {
    fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String> {
        CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        let a = ctx.input.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
        let b = ctx.input.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
        Ok(ToolOutput::json(json!({ "sum": a + b })))
    }
}

fn unique_id(label: &str) -> String {
    format!("integration_test_{}_{}", label, std::process::id())
}

#[test]
fn tool_output_json_wraps_value_with_no_state_update() {
    let out = ToolOutput::json(json!({"answer": 42}));
    assert_eq!(out.result, json!({"answer": 42}));
    assert!(out.state_update.is_none());
}

#[test]
fn tool_output_state_update_round_trip() {
    let out = ToolOutput {
        result: json!("result"),
        state_update: Some(json!({"k": "v"})),
    };
    assert_eq!(out.state_update.as_ref(), Some(&json!({"k": "v"})));
}

#[test]
fn get_tool_returns_none_for_unknown_id() {
    assert!(get_tool("definitely_not_registered").is_none());
}

#[test]
fn tool_count_reflects_registrations() {
    let id = unique_id("count_test");
    let before = tool_count();
    register_tool_as(&id, TestAddTool);
    // Use `>=` rather than `== before + 1` because a parallel test in
    // the same binary may have registered another tool between our
    // `before` snapshot and our `after` call.
    let after = tool_count();
    assert!(
        after >= before + 1,
        "register_tool_as should bump the count: before={before} after={after}"
    );
    assert!(
        get_tool(&id).is_some(),
        "our just-registered tool should be retrievable"
    );
}

#[test]
fn list_tools_includes_registered_id() {
    let id = unique_id("list_test");
    register_tool_as(&id, TestAddTool);
    // The registry is process-global and other tests in this binary may
    // add or clear entries concurrently. We only assert that *our* id
    // is present after we register it; we never assert it remains after
    // a parallel test runs.
    let tools = list_tools();
    assert!(
        tools.contains(&id),
        "list_tools should include {id}, got {tools:?}"
    );
    // Do NOT clean up — a parallel test may have already cleared the
    // registry between our register and this call. Leaving the entry
    // behind is harmless because the id is unique to this process.
}

#[test]
fn register_overwrites_previous_tool_with_same_id() {
    let id = unique_id("overwrite");
    register_tool_as(&id, TestAddTool);
    let before_count = tool_count();
    register_tool_as(&id, TestAddTool); // same id
    let after_count = tool_count();
    assert_eq!(
        before_count, after_count,
        "re-registering with same id should not add a new entry"
    );
    assert!(get_tool(&id).is_some());
    remove_tool(&id);
}

#[test]
fn remove_tool_returns_false_for_unknown_id() {
    let result = remove_tool("definitely_not_registered_for_removal");
    assert!(!result, "removing an unknown tool should return false");
}

#[test]
fn registered_tool_can_be_invoked() {
    let id = unique_id("invoke_test");
    register_tool_as(&id, TestAddTool);

    let tool = get_tool(&id).expect("registered");
    // trait has no name() method in this version; just verify it was registered

    let ctx = ToolContext {
        tool_id: "test-add".to_string(),
        input: json!({"a": 2, "b": 3}),
        state: Value::Null,
    };
    let out = tool.execute(&ctx).expect("execute");
    assert_eq!(out.result, json!({"sum": 5}));

    remove_tool(&id);
}

#[test]
fn json_value_input_is_preserved() {
    // The ToolContext.input field is public; verify that nested JSON
    // survives a round-trip via the SDK types.
    let ctx = ToolContext {
        tool_id: "test".to_string(),
        input: json!({
            "nested": {"a": [1, 2, 3], "b": "string"},
            "flag": true
        }),
        state: json!({"prev": 7}),
    };
    assert_eq!(ctx.input["nested"]["a"][0], json!(1));
    assert_eq!(ctx.input["nested"]["b"], json!("string"));
    assert_eq!(ctx.input["flag"], json!(true));
    assert_eq!(ctx.state["prev"], json!(7));
}
