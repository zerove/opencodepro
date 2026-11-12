//! Integration tests for sisyphus-tools-mcp `mcp::types`.
//!
//! Exercises the public JSON-RPC 2.0 and MCP protocol types:
//! request/response/error/notification envelopes, and the
//! initialize handshake types. All types are pure data with
//! serde derives, so the tests focus on serialization round-trips
//! and field semantics.

use sisyphus_tools_mcp::mcp::types::{
    ClientCapabilities, ClientInfo, InitializeParams, InitializeResult, JsonRpcError,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, ResourcesCapability, ServerCapabilities,
    ServerInfo, ToolsCapability,
};

// ---------------------------------------------------------------------------
// JsonRpcRequest
// ---------------------------------------------------------------------------

#[test]
fn request_serializes_with_jsonrpc_version() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(1),
        method: "tools/list".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&req).expect("ok");
    assert!(json.contains("\"jsonrpc\":\"2.0\""));
    assert!(json.contains("\"method\":\"tools/list\""));
    assert!(json.contains("\"id\":1"));
}

#[test]
fn request_roundtrip_preserves_all_fields() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(42),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({"name": "echo", "args": {"x": 1}})),
    };
    let json = serde_json::to_string(&req).expect("");
    let parsed: JsonRpcRequest = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.jsonrpc, "2.0");
    assert_eq!(parsed.id, Some(42));
    assert_eq!(parsed.method, "tools/call");
    assert!(parsed.params.is_some());
}

#[test]
fn request_with_no_id_serializes_without_id_field() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: None,
        method: "notify".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&req).expect("");
    // skip_serializing_if = Option::is_none omits None fields.
    assert!(!json.contains("\"id\""));
}

// ---------------------------------------------------------------------------
// JsonRpcResponse
// ---------------------------------------------------------------------------

#[test]
fn response_with_result_serializes() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: Some(1),
        result: Some(serde_json::json!({"ok": true})),
        error: None,
    };
    let json = serde_json::to_string(&resp).expect("");
    assert!(json.contains("\"result\""));
    assert!(!json.contains("\"error\""));
}

#[test]
fn response_with_error_serializes() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: Some(1),
        result: None,
        error: Some(JsonRpcError {
            code: -32600,
            message: "Invalid request".to_string(),
            data: None,
        }),
    };
    let json = serde_json::to_string(&resp).expect("");
    assert!(json.contains("\"error\""));
    assert!(!json.contains("\"result\""));
    assert!(json.contains("\"code\":-32600"));
}

#[test]
fn response_roundtrip_preserves_error() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: Some(99),
        result: None,
        error: Some(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: Some(serde_json::json!({"method": "unknown"})),
        }),
    };
    let json = serde_json::to_string(&resp).expect("");
    let parsed: JsonRpcResponse = serde_json::from_str(&json).expect("");
    let err = parsed.error.expect("error present");
    assert_eq!(err.code, -32601);
    assert_eq!(err.message, "Method not found");
    assert!(err.data.is_some());
}

// ---------------------------------------------------------------------------
// JsonRpcNotification
// ---------------------------------------------------------------------------

#[test]
fn notification_has_no_id_field() {
    let n = JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "notifications/initialized".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&n).expect("");
    assert!(!json.contains("\"id\""));
    assert!(json.contains("\"method\":\"notifications/initialized\""));
}

#[test]
fn notification_roundtrip() {
    let n = JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "tools/changed".to_string(),
        params: Some(serde_json::json!({"count": 5})),
    };
    let json = serde_json::to_string(&n).expect("");
    let parsed: JsonRpcNotification = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.method, "tools/changed");
    assert_eq!(parsed.params.unwrap()["count"], 5);
}

// ---------------------------------------------------------------------------
// InitializeParams
// ---------------------------------------------------------------------------

#[test]
fn initialize_params_with_all_fields() {
    let p = InitializeParams {
        protocol_version: Some("2024-11-05".into()),
        capabilities: Some(ClientCapabilities {
            tools: Some(ToolsCapability {
                list_changed: Some(true),
            }),
            resources: Some(ResourcesCapability {
                subscribe: Some(false),
                list_changed: Some(true),
            }),
            prompts: None,
        }),
        client_info: Some(ClientInfo {
            name: "test_client".into(),
            version: "1.0.0".into(),
        }),
    };
    let json = serde_json::to_string(&p).expect("");
    let parsed: InitializeParams = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.protocol_version.as_deref(), Some("2024-11-05"));
    let info = parsed.client_info.expect("info");
    assert_eq!(info.name, "test_client");
    assert_eq!(info.version, "1.0.0");
}

#[test]
fn initialize_params_minimal_serializes() {
    let p = InitializeParams {
        protocol_version: None,
        capabilities: None,
        client_info: None,
    };
    let json = serde_json::to_string(&p).expect("");
    // All None fields skipped.
    assert!(!json.contains("\"protocolVersion\""));
    assert!(!json.contains("\"capabilities\""));
    assert!(!json.contains("\"clientInfo\""));
}

// ---------------------------------------------------------------------------
// InitializeResult
// ---------------------------------------------------------------------------

#[test]
fn initialize_result_with_server_info() {
    let r = InitializeResult {
        protocol_version: "2024-11-05".into(),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: Some(false),
            }),
            resources: None,
            prompts: None,
        },
        server_info: Some(ServerInfo {
            name: "my_server".into(),
            version: "2.0.0".into(),
        }),
    };
    let json = serde_json::to_string(&r).expect("");
    let parsed: InitializeResult = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.protocol_version, "2024-11-05");
    let info = parsed.server_info.expect("present");
    assert_eq!(info.name, "my_server");
    assert_eq!(info.version, "2.0.0");
}

// ---------------------------------------------------------------------------
// Default JSON-RPC version
// ---------------------------------------------------------------------------

#[test]
fn missing_jsonrpc_field_defaults_to_2_0() {
    // If jsonrpc is omitted, serde should default to "2.0".
    let json = r#"{"id":1,"method":"foo"}"#;
    let req: JsonRpcRequest = serde_json::from_str(json).expect("");
    assert_eq!(req.jsonrpc, "2.0");
    assert_eq!(req.method, "foo");
}
