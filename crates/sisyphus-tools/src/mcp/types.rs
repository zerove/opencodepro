use serde::{Deserialize, Serialize};

// ── JSON-RPC 2.0 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

fn default_jsonrpc() -> String {
    "2.0".to_string()
}

// ── MCP Protocol Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ClientCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_info: Option<ClientInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_info: Option<ServerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_jsonrpc() {
        assert_eq!(default_jsonrpc(), "2.0");
    }

    #[test]
    fn test_json_rpc_request_roundtrip() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(1),
            method: "ping".into(),
            params: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.method, "ping");
        assert_eq!(back.id, Some(1));
    }

    #[test]
    fn test_json_rpc_request_no_id() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notify".into(),
            params: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("\"id\""), "no id field should be serialized");
    }

    #[test]
    fn test_json_rpc_response_ok() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(42),
            result: Some(serde_json::json!({"ok": true})),
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, Some(42));
        assert!(back.result.is_some());
        assert!(back.error.is_none());
    }

    #[test]
    fn test_json_rpc_response_error() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(1),
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "method not found".into(),
                data: None,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        let err = back.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "method not found");
    }

    #[test]
    fn test_json_rpc_error_with_data() {
        let err = JsonRpcError {
            code: -32000,
            message: "tool error".into(),
            data: Some(serde_json::json!({"details": "timeout"})),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: JsonRpcError = serde_json::from_str(&json).unwrap();
        assert!(back.data.is_some());
        assert_eq!(back.data.unwrap()["details"], "timeout");
    }

    #[test]
    fn test_json_rpc_notification_roundtrip() {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0".into(),
            method: "cancelled".into(),
            params: Some(serde_json::json!({"taskId": "t1"})),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let back: JsonRpcNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(back.method, "cancelled");
        assert_eq!(back.params.unwrap()["taskId"], "t1");
    }

    #[test]
    fn test_mcp_tool_roundtrip() {
        let tool = McpTool {
            name: "read_file".into(),
            description: Some("Read a file".into()),
            input_schema: Some(serde_json::json!({"type": "object"})),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: McpTool = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "read_file");
        assert_eq!(back.description.unwrap(), "Read a file");
    }

    #[test]
    fn test_mcp_tool_no_optional_fields() {
        let tool = McpTool {
            name: "ping".into(),
            description: None,
            input_schema: None,
        };
        let json = serde_json::to_string(&tool).unwrap();
        // optional fields should be skipped
        assert!(!json.contains("description"));
        let back: McpTool = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "ping");
        assert!(back.description.is_none());
    }

    #[test]
    fn test_call_tool_params_roundtrip() {
        let params = CallToolParams {
            name: "execute".into(),
            arguments: Some(serde_json::json!({"cmd": "ls"})),
        };
        let json = serde_json::to_string(&params).unwrap();
        let back: CallToolParams = serde_json::from_str(&json).unwrap();
        assert_eq!(back.arguments.unwrap()["cmd"], "ls");
    }

    #[test]
    fn test_call_tool_result_text() {
        let result = CallToolResult {
            content: vec![ToolContent::Text { text: "hello".into() }],
            is_error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: CallToolResult = serde_json::from_str(&json).unwrap();
        match &back.content[0] {
            ToolContent::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn test_call_tool_result_resource() {
        let result = CallToolResult {
            content: vec![ToolContent::Resource {
                resource: ResourceContent {
                    uri: "file:///tmp/x".into(),
                    mime_type: Some("text/plain".into()),
                    text: "content".into(),
                },
            }],
            is_error: Some(true),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: CallToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.is_error, Some(true));
        match &back.content[0] {
            ToolContent::Resource { resource } => {
                assert_eq!(resource.uri, "file:///tmp/x");
            }
            _ => panic!("expected Resource variant"),
        }
    }

    #[test]
    fn test_initialize_params_roundtrip() {
        let params = InitializeParams {
            protocol_version: Some("2025-03-26".into()),
            capabilities: Some(ClientCapabilities {
                tools: Some(ToolsCapability { list_changed: Some(true) }),
                resources: None,
            }),
            client_info: Some(ClientInfo {
                name: "sisyphus".into(),
                version: "0.1".into(),
            }),
        };
        let json = serde_json::to_string_pretty(&params).unwrap();
        let back: InitializeParams = serde_json::from_str(&json).unwrap();
        assert_eq!(back.protocol_version.unwrap(), "2025-03-26");
        assert_eq!(back.client_info.unwrap().name, "sisyphus");
    }

    #[test]
    fn test_initialize_result_roundtrip() {
        let result = InitializeResult {
            protocol_version: "2025-03-26".into(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability { list_changed: None }),
                resources: Some(ResourcesCapability {
                    subscribe: Some(false),
                    list_changed: None,
                }),
            },
            server_info: Some(ServerInfo {
                name: "test-server".into(),
                version: "1.0".into(),
            }),
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let back: InitializeResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.protocol_version, "2025-03-26");
        assert!(back.capabilities.resources.is_some());
    }

    #[test]
    fn test_mcp_error_code_constants() {
        assert_eq!(MCP_ERROR_PARSE, -32700);
        assert_eq!(MCP_ERROR_INVALID_REQUEST, -32600);
        assert_eq!(MCP_ERROR_METHOD_NOT_FOUND, -32601);
        assert_eq!(MCP_ERROR_INVALID_PARAMS, -32602);
        assert_eq!(MCP_ERROR_INTERNAL, -32603);
        assert_eq!(MCP_ERROR_TOOL_NOT_FOUND, -32000);
        assert_eq!(MCP_ERROR_TOOL_EXECUTION, -32001);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "resource")]
    Resource {
        resource: ResourceContent,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContent {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub text: String,
}

// ── MCP Error Codes ──

pub const MCP_ERROR_PARSE: i64 = -32700;
pub const MCP_ERROR_INVALID_REQUEST: i64 = -32600;
pub const MCP_ERROR_METHOD_NOT_FOUND: i64 = -32601;
pub const MCP_ERROR_INVALID_PARAMS: i64 = -32602;
pub const MCP_ERROR_INTERNAL: i64 = -32603;

pub const MCP_ERROR_TOOL_NOT_FOUND: i64 = -32000;
pub const MCP_ERROR_TOOL_EXECUTION: i64 = -32001;
