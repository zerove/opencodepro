//! Integration tests for sisyphus-tools-mcp `mcp::types` module.
//!
//! Exercises the pure-data types in the JSON-RPC and MCP protocol
//! surface: [`JsonRpcRequest`], [`JsonRpcResponse`], [`JsonRpcError`],
//! [`JsonRpcNotification`], and the Initialize* types.

use sisyphus_tools_mcp::mcp::types::{
    ClientCapabilities, ClientInfo, InitializeParams, InitializeResult, JsonRpcError,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, ServerCapabilities, ServerInfo,
};

// ---------------------------------------------------------------------------
// JsonRpcRequest
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_request_field_assignment() {
    let r = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(1),
        method: "tools/list".into(),
        params: Some(serde_json::json!({})),
    };
    assert_eq!(r.jsonrpc, "2.0");
    assert_eq!(r.id, Some(1));
    assert_eq!(r.method, "tools/list");
}

#[test]
fn json_rpc_request_serde() {
    let r = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(42),
        method: "ping".into(),
        params: None,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: JsonRpcRequest = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, Some(42));
    assert_eq!(back.method, "ping");
}

#[test]
fn json_rpc_request_serde_with_params() {
    let r = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: None,
        method: "notification".into(),
        params: Some(serde_json::json!({"key": "value"})),
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: JsonRpcRequest = serde_json::from_str(&j).unwrap();
    assert!(back.id.is_none());
    assert_eq!(back.params.unwrap()["key"], "value");
}

// ---------------------------------------------------------------------------
// JsonRpcResponse
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_response_success() {
    let r = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: Some(1),
        result: Some(serde_json::json!({"ok": true})),
        error: None,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: JsonRpcResponse = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, Some(1));
    assert!(back.result.is_some());
    assert!(back.error.is_none());
}

#[test]
fn json_rpc_response_error() {
    let r = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: Some(1),
        result: None,
        error: Some(JsonRpcError {
            code: -32600,
            message: "invalid request".into(),
            data: None,
        }),
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: JsonRpcResponse = serde_json::from_str(&j).unwrap();
    assert!(back.result.is_none());
    let err = back.error.unwrap();
    assert_eq!(err.code, -32600);
    assert_eq!(err.message, "invalid request");
}

// ---------------------------------------------------------------------------
// JsonRpcError
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_error_field_assignment() {
    let e = JsonRpcError {
        code: -1,
        message: "x".into(),
        data: Some(serde_json::json!("extra")),
    };
    assert_eq!(e.code, -1);
    assert_eq!(e.message, "x");
    assert!(e.data.is_some());
}

#[test]
fn json_rpc_error_serde_round_trip() {
    let e = JsonRpcError {
        code: 42,
        message: "m".into(),
        data: None,
    };
    let j = serde_json::to_string(&e).unwrap();
    let back: JsonRpcError = serde_json::from_str(&j).unwrap();
    assert_eq!(back.code, 42);
}

// ---------------------------------------------------------------------------
// JsonRpcNotification
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_notification_field_assignment() {
    let n = JsonRpcNotification {
        jsonrpc: "2.0".into(),
        method: "notify".into(),
        params: Some(serde_json::json!({"k": 1})),
    };
    assert_eq!(n.method, "notify");
}

#[test]
fn json_rpc_notification_serde() {
    let n = JsonRpcNotification {
        jsonrpc: "2.0".into(),
        method: "x".into(),
        params: None,
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: JsonRpcNotification = serde_json::from_str(&j).unwrap();
    assert_eq!(back.method, "x");
    assert!(back.params.is_none());
}

// ---------------------------------------------------------------------------
// InitializeParams / Result
// ---------------------------------------------------------------------------

#[test]
fn initialize_params_field_assignment() {
    let p = InitializeParams {
        protocol_version: Some("2024-11-05".into()),
        capabilities: Some(ClientCapabilities {
            tools: None,
            resources: None,
        }),
        client_info: Some(ClientInfo {
            name: "test".into(),
            version: "1.0.0".into(),
        }),
    };
    assert_eq!(p.protocol_version.as_deref(), Some("2024-11-05"));
    assert_eq!(p.client_info.as_ref().unwrap().name, "test");
}

#[test]
fn initialize_params_all_none() {
    let p = InitializeParams {
        protocol_version: None,
        capabilities: None,
        client_info: None,
    };
    assert!(p.protocol_version.is_none());
    assert!(p.capabilities.is_none());
    assert!(p.client_info.is_none());
}

#[test]
fn initialize_result_field_assignment() {
    let r = InitializeResult {
        protocol_version: "2024-11-05".into(),
        capabilities: ServerCapabilities {
            tools: None,
            resources: None,
        },
        server_info: Some(ServerInfo {
            name: "srv".into(),
            version: "0.1.0".into(),
        }),
    };
    assert!(r.server_info.is_some());
    let s = r.server_info.unwrap();
    assert_eq!(s.name, "srv");
    assert_eq!(s.version, "0.1.0");
}

#[test]
fn initialize_result_serde_no_server_info() {
    let r = InitializeResult {
        protocol_version: "v1".into(),
        capabilities: ServerCapabilities {
            tools: None,
            resources: None,
        },
        server_info: None,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: InitializeResult = serde_json::from_str(&j).unwrap();
    assert_eq!(back.protocol_version, "v1");
    assert!(back.server_info.is_none());
}
