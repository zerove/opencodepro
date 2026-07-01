use super::transport::McpTransport;
use super::types::{ServerInfo, ServerCapabilities, JsonRpcResponse, JsonRpcRequest, JsonRpcNotification, InitializeResult, InitializeParams, ClientCapabilities, ToolsCapability, ResourcesCapability, ClientInfo, McpTool, CallToolResult, CallToolParams};
use crate::ToolResult;

/// High-level MCP client.
pub struct McpClient {
    transport: Box<dyn McpTransport>,
    next_id: u64,
    server_info: Option<ServerInfo>,
    server_capabilities: ServerCapabilities,
}

impl McpClient {
    /// Create a new MCP client with the given transport.
    pub fn new(transport: Box<dyn McpTransport>) -> Self {
        Self {
            transport,
            next_id: 1,
            server_info: None,
            server_capabilities: ServerCapabilities {
                tools: None,
                resources: None,
            },
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Send a raw JSON-RPC request and get the response.
    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> ToolResult<JsonRpcResponse> {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: method.to_string(),
            params,
        };
        let msg = serde_json::to_string(&req)
            .map_err(|e| crate::ToolError::TodoError(format!("serialize request: {e}")))?;
        let resp_text = self.transport.request(&msg).await?;
        let resp: JsonRpcResponse = serde_json::from_str(&resp_text)
            .map_err(|e| crate::ToolError::TodoError(format!("deserialize response: {e}")))?;
        Ok(resp)
    }


    /// Send a notification (no response expected).
    async fn send_notification(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> ToolResult<()> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        };
        let msg = serde_json::to_string(&notification)
            .map_err(|e| crate::ToolError::TodoError(format!("serialize notification: {e}")))?;
        self.transport.request(&msg).await?;
        Ok(())
    }

    /// Initialize the MCP session.
    pub async fn initialize(
        &mut self,
        client_name: &str,
        client_version: &str,
    ) -> ToolResult<InitializeResult> {
        let params = InitializeParams {
            protocol_version: Some("2025-03-26".to_string()),
            capabilities: Some(ClientCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(true),
                }),
                resources: Some(ResourcesCapability {
                    subscribe: Some(false),
                    list_changed: Some(false),
                }),
            }),
            client_info: Some(ClientInfo {
                name: client_name.to_string(),
                version: client_version.to_string(),
            }),
        };

        let resp = self
            .send_request(
                "initialize",
                Some(
                    serde_json::to_value(params).map_err(|e| {
                        crate::ToolError::TodoError(format!("serialize params: {e}"))
                    })?,
                ),
            )
            .await?;

        if let Some(e) = resp.error {
            return Err(crate::ToolError::TodoError(format!(
                "initialize error ({}): {}",
                e.code, e.message
            )));
        }

        let result: InitializeResult = serde_json::from_value(resp.result.ok_or_else(|| {
            crate::ToolError::TodoError("initialize: missing result".to_string())
        })?)
        .map_err(|e| crate::ToolError::TodoError(format!("deserialize result: {e}")))?;

        self.server_info = result.server_info.clone();
        self.server_capabilities = result.capabilities.clone();

        // Send initialized notification
        self.send_notification("notifications/initialized", None)
            .await?;

        Ok(result)
    }

    /// List tools available on the MCP server.
    pub async fn list_tools(&mut self) -> ToolResult<Vec<McpTool>> {
        let resp = self.send_request("tools/list", None).await?;

        if let Some(e) = resp.error {
            return Err(crate::ToolError::TodoError(format!(
                "list_tools error ({}): {}",
                e.code, e.message
            )));
        }

        let result = resp
            .result
            .ok_or_else(|| crate::ToolError::TodoError("list_tools: missing result".to_string()))?;

        #[derive(serde::Deserialize)]
        struct ToolsListResult {
            tools: Vec<McpTool>,
        }

        let parsed: ToolsListResult = serde_json::from_value(result)
            .map_err(|e| crate::ToolError::TodoError(format!("deserialize tools: {e}")))?;

        Ok(parsed.tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> ToolResult<CallToolResult> {
        let params = CallToolParams {
            name: name.to_string(),
            arguments,
        };

        let resp = self
            .send_request(
                "tools/call",
                Some(
                    serde_json::to_value(params).map_err(|e| {
                        crate::ToolError::TodoError(format!("serialize params: {e}"))
                    })?,
                ),
            )
            .await?;

        if let Some(e) = resp.error {
            return Err(crate::ToolError::TodoError(format!(
                "call_tool error ({}): {}",
                e.code, e.message
            )));
        }

        let result: CallToolResult =
            serde_json::from_value(resp.result.ok_or_else(|| {
                crate::ToolError::TodoError("call_tool: missing result".to_string())
            })?)
            .map_err(|e| crate::ToolError::TodoError(format!("deserialize result: {e}")))?;

        Ok(result)
    }

    /// Get the server info (available after initialize).
    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.server_info.as_ref()
    }

    /// Close the transport.
    pub async fn close(&mut self) -> ToolResult<()> {
        self.transport.close().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolResult;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    /// Mock transport that returns pre-defined responses.
    struct MockTransport {
        responses: Arc<Mutex<Vec<String>>>,
    }

    impl MockTransport {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses)),
            }
        }
    }

    #[async_trait]
    impl McpTransport for MockTransport {
        async fn request(&mut self, _message: &str) -> ToolResult<String> {
            let mut list = self.responses.lock().unwrap();
            if list.is_empty() {
                Err(crate::ToolError::TodoError(
                    "no more mock responses".to_string(),
                ))
            } else {
                Ok(list.remove(0))
            }
        }

        async fn close(&mut self) -> ToolResult<()> {
            Ok(())
        }
    }

    fn mock_initialize_response() -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocol_version": "2025-03-26",
                "capabilities": {
                    "tools": { "list_changed": true },
                    "resources": { "subscribe": false, "list_changed": false }
                },
                "server_info": { "name": "test-server", "version": "1.0.0" }
            }
        })
        .to_string()
    }

    fn mock_tools_list_response() -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "tools": [
                    { "name": "echo", "description": "Echo input back", "input_schema": { "type": "object", "properties": { "text": { "type": "string" } } } },
                    { "name": "add", "description": "Add two numbers", "input_schema": { "type": "object", "properties": { "a": { "type": "number" }, "b": { "type": "number" } } } }
                ]
            }
        }).to_string()
    }

    fn mock_tool_call_response() -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": {
                "content": [{ "type": "text", "text": "hello back" }],
                "is_error": false
            }
        })
        .to_string()
    }

    fn mock_initialize_response_for_notification() -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocol_version": "2025-03-26",
                "capabilities": { "tools": null, "resources": null },
                "server_info": { "name": "test", "version": "0.1" }
            }
        })
        .to_string()
    }

    #[tokio::test]
    async fn test_initialize() {
        let transport = MockTransport::new(vec![mock_initialize_response(), "{}".to_string()]);
        let mut client = McpClient::new(Box::new(transport));
        let result = client.initialize("test-client", "0.1.0").await.unwrap();
        assert_eq!(result.protocol_version, "2025-03-26");
        assert_eq!(result.server_info.as_ref().unwrap().name, "test-server");
    }

    #[tokio::test]
    async fn test_initialize_error() {
        let transport = MockTransport::new(vec![json!({
            "jsonrpc": "2.0", "id": 1,
            "error": { "code": -32603, "message": "internal error" }
        })
        .to_string()]);
        let mut client = McpClient::new(Box::new(transport));
        let result = client.initialize("test", "0.1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_tools() {
        let transport = MockTransport::new(vec![
            mock_initialize_response_for_notification(),
            "{}".to_string(),
            mock_tools_list_response(),
        ]);
        let mut client = McpClient::new(Box::new(transport));
        client.initialize("test", "0.1").await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[1].name, "add");
    }

    #[tokio::test]
    async fn test_call_tool() {
        let transport = MockTransport::new(vec![
            mock_initialize_response_for_notification(),
            "{}".to_string(),
            mock_tool_call_response(),
        ]);
        let mut client = McpClient::new(Box::new(transport));
        client.initialize("test", "0.1").await.unwrap();
        let args = json!({ "text": "hello" });
        let result = client.call_tool("echo", Some(args)).await.unwrap();
        assert_eq!(result.content.len(), 1);
        assert!(!result.is_error.unwrap_or(false));
    }

    #[tokio::test]
    async fn test_call_tool_error() {
        let transport = MockTransport::new(vec![
            mock_initialize_response_for_notification(),
            "{}".to_string(),
            json!({
                "jsonrpc": "2.0", "id": 3,
                "error": { "code": -32001, "message": "execution failed" }
            })
            .to_string(),
        ]);
        let mut client = McpClient::new(Box::new(transport));
        client.initialize("test", "0.1").await.unwrap();
        let result = client.call_tool("bad_tool", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_server_info() {
        let transport = MockTransport::new(vec![mock_initialize_response(), "{}".to_string()]);
        let mut client = McpClient::new(Box::new(transport));
        assert!(client.server_info().is_none());
        client.initialize("test", "0.1").await.unwrap();
        assert_eq!(client.server_info().unwrap().name, "test-server");
    }
}
