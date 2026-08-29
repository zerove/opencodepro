//! Integration tests for sisyphus-tools-mcp `mcp::types` (call/tool
//! content) module. Covers [`McpTool`], [`CallToolParams`],
//! [`CallToolResult`], [`ToolContent`], and [`ResourceContent`].

use sisyphus_tools_mcp::mcp::types::{
    CallToolParams, CallToolResult, McpTool, ResourceContent, ToolContent,
};

// ---------------------------------------------------------------------------
// McpTool
// ---------------------------------------------------------------------------

#[test]
fn mcp_tool_field_assignment() {
    let t = McpTool {
        name: "read".into(),
        description: Some("read a file".into()),
        input_schema: Some(serde_json::json!({"type": "object"})),
    };
    assert_eq!(t.name, "read");
    assert!(t.description.is_some());
}

#[test]
fn mcp_tool_minimal() {
    let t = McpTool {
        name: "ping".into(),
        description: None,
        input_schema: None,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: McpTool = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "ping");
    assert!(back.description.is_none());
}

// ---------------------------------------------------------------------------
// CallToolParams
// ---------------------------------------------------------------------------

#[test]
fn call_tool_params_field_assignment() {
    let p = CallToolParams {
        name: "read".into(),
        arguments: Some(serde_json::json!({"path": "/etc/passwd"})),
    };
    assert_eq!(p.name, "read");
    let args = p.arguments.unwrap();
    assert_eq!(args["path"], "/etc/passwd");
}

#[test]
fn call_tool_params_no_args() {
    let p = CallToolParams {
        name: "ping".into(),
        arguments: None,
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: CallToolParams = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "ping");
}

// ---------------------------------------------------------------------------
// CallToolResult
// ---------------------------------------------------------------------------

#[test]
fn call_tool_result_text_content() {
    let r = CallToolResult {
        content: vec![ToolContent::Text {
            text: "hello".into(),
        }],
        is_error: Some(false),
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: CallToolResult = serde_json::from_str(&j).unwrap();
    assert_eq!(back.content.len(), 1);
    assert!(!back.is_error.unwrap());
}

#[test]
fn call_tool_result_resource_content() {
    let r = CallToolResult {
        content: vec![ToolContent::Resource {
            resource: ResourceContent {
                uri: "file:///x".into(),
                mime_type: Some("text/plain".into()),
                text: "body".into(),
            },
        }],
        is_error: None,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: CallToolResult = serde_json::from_str(&j).unwrap();
    assert_eq!(back.content.len(), 1);
    match &back.content[0] {
        ToolContent::Resource { resource } => {
            assert_eq!(resource.uri, "file:///x");
            assert_eq!(resource.text, "body");
        }
        _ => panic!("expected Resource variant"),
    }
}

#[test]
fn call_tool_result_empty_content() {
    let r = CallToolResult {
        content: vec![],
        is_error: None,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: CallToolResult = serde_json::from_str(&j).unwrap();
    assert!(back.content.is_empty());
}

#[test]
fn call_tool_result_is_error_true() {
    let r = CallToolResult {
        content: vec![ToolContent::Text {
            text: "boom".into(),
        }],
        is_error: Some(true),
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: CallToolResult = serde_json::from_str(&j).unwrap();
    assert!(back.is_error.unwrap());
}

// ---------------------------------------------------------------------------
// ResourceContent
// ---------------------------------------------------------------------------

#[test]
fn resource_content_field_assignment() {
    let r = ResourceContent {
        uri: "file:///etc/hosts".into(),
        mime_type: None,
        text: "127.0.0.1 localhost".into(),
    };
    assert_eq!(r.uri, "file:///etc/hosts");
    assert!(r.mime_type.is_none());
    assert_eq!(r.text, "127.0.0.1 localhost");
}

#[test]
fn resource_content_serde() {
    let r = ResourceContent {
        uri: "x".into(),
        mime_type: Some("text/plain".into()),
        text: "y".into(),
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: ResourceContent = serde_json::from_str(&j).unwrap();
    assert_eq!(back.uri, "x");
    assert_eq!(back.mime_type.as_deref(), Some("text/plain"));
}
