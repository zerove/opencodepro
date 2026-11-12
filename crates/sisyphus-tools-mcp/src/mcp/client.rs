use super::transport::McpTransport;
use super::types::{
    CallToolParams, CallToolResult, ClientCapabilities, ClientInfo, GetPromptParams,
    GetPromptResult, InitializeParams, InitializeResult, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, ListPromptsResult, ListResourcesResult, McpTool, PromptsCapability,
    ReadResourceParams, ReadResourceResult, ResourcesCapability, ServerCapabilities, ServerInfo,
    ToolsCapability,
};
use metrics::counter;
use sisyphus_tools_core::ToolResult;

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
                prompts: None,
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
        let msg = serde_json::to_string(&req).map_err(|e| {
            sisyphus_tools_core::ToolError::TodoError(format!("serialize request: {e}"))
        })?;
        let resp_text = self.transport.request(&msg).await?;
        let resp: JsonRpcResponse = serde_json::from_str(&resp_text).map_err(|e| {
            sisyphus_tools_core::ToolError::TodoError(format!("deserialize response: {e}"))
        })?;
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
        let msg = serde_json::to_string(&notification).map_err(|e| {
            sisyphus_tools_core::ToolError::TodoError(format!("serialize notification: {e}"))
        })?;
        // 通知不会收到响应：必须走 notify（只写不读），否则会阻塞到超时。
        self.transport.notify(&msg).await?;
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
                prompts: Some(PromptsCapability {
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
                Some(serde_json::to_value(params).map_err(|e| {
                    sisyphus_tools_core::ToolError::TodoError(format!("serialize params: {e}"))
                })?),
            )
            .await?;

        if let Some(e) = resp.error {
            return Err(sisyphus_tools_core::ToolError::TodoError(format!(
                "initialize error ({}): {}",
                e.code, e.message
            )));
        }

        let result: InitializeResult = serde_json::from_value(resp.result.ok_or_else(|| {
            sisyphus_tools_core::ToolError::TodoError("initialize: missing result".to_string())
        })?)
        .map_err(|e| {
            sisyphus_tools_core::ToolError::TodoError(format!("deserialize result: {e}"))
        })?;

        self.server_info = result.server_info.clone();
        self.server_capabilities = result.capabilities.clone();

        // Send initialized notification
        self.send_notification("notifications/initialized", None).await?;

        Ok(result)
    }

    /// List tools available on the MCP server.
    pub async fn list_tools(&mut self) -> ToolResult<Vec<McpTool>> {
        let resp = self.send_request("tools/list", None).await?;

        if let Some(e) = resp.error {
            return Err(sisyphus_tools_core::ToolError::TodoError(format!(
                "list_tools error ({}): {}",
                e.code, e.message
            )));
        }

        let result = resp.result.ok_or_else(|| {
            sisyphus_tools_core::ToolError::TodoError("list_tools: missing result".to_string())
        })?;

        #[derive(serde::Deserialize)]
        struct ToolsListResult {
            tools: Vec<McpTool>,
        }

        let parsed: ToolsListResult = serde_json::from_value(result).map_err(|e| {
            sisyphus_tools_core::ToolError::TodoError(format!("deserialize tools: {e}"))
        })?;

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
                Some(serde_json::to_value(params).map_err(|e| {
                    sisyphus_tools_core::ToolError::TodoError(format!("serialize params: {e}"))
                })?),
            )
            .await?;

        if let Some(e) = resp.error {
            return Err(sisyphus_tools_core::ToolError::TodoError(format!(
                "call_tool error ({}): {}",
                e.code, e.message
            )));
        }

        let result: CallToolResult = serde_json::from_value(resp.result.ok_or_else(|| {
            sisyphus_tools_core::ToolError::TodoError("call_tool: missing result".to_string())
        })?)
        .map_err(|e| {
            sisyphus_tools_core::ToolError::TodoError(format!("deserialize result: {e}"))
        })?;

        counter!(
            "sisyphus_mcp_tool_calls_total",
            "tool" => name.to_string(),
        )
        .increment(1);
        Ok(result)
    }

    /// List resources available on the MCP server.
    pub async fn list_resources(&mut self) -> ToolResult<ListResourcesResult> {
        let resp = self.send_request("resources/list", None).await?;
        if let Some(e) = resp.error {
            return Err(sisyphus_tools_core::ToolError::TodoError(format!(
                "list_resources error ({}): {}",
                e.code, e.message
            )));
        }
        let result = resp.result.ok_or_else(|| {
            sisyphus_tools_core::ToolError::TodoError("list_resources: missing result".to_string())
        })?;
        let parsed: ListResourcesResult = serde_json::from_value(result).map_err(|e| {
            sisyphus_tools_core::ToolError::TodoError(format!("deserialize resources: {e}"))
        })?;
        Ok(parsed)
    }

    /// Read a resource from the MCP server by URI.
    pub async fn read_resource(&mut self, uri: &str) -> ToolResult<ReadResourceResult> {
        let params = ReadResourceParams {
            uri: uri.to_string(),
        };
        let resp = self
            .send_request(
                "resources/read",
                Some(serde_json::to_value(params).map_err(|e| {
                    sisyphus_tools_core::ToolError::TodoError(format!("serialize params: {e}"))
                })?),
            )
            .await?;
        if let Some(e) = resp.error {
            return Err(sisyphus_tools_core::ToolError::TodoError(format!(
                "read_resource error ({}): {}",
                e.code, e.message
            )));
        }
        let result = resp.result.ok_or_else(|| {
            sisyphus_tools_core::ToolError::TodoError("read_resource: missing result".to_string())
        })?;
        let parsed: ReadResourceResult = serde_json::from_value(result).map_err(|e| {
            sisyphus_tools_core::ToolError::TodoError(format!("deserialize resource: {e}"))
        })?;
        Ok(parsed)
    }

    /// List prompts available on the MCP server.
    pub async fn list_prompts(&mut self) -> ToolResult<ListPromptsResult> {
        let resp = self.send_request("prompts/list", None).await?;
        if let Some(e) = resp.error {
            return Err(sisyphus_tools_core::ToolError::TodoError(format!(
                "list_prompts error ({}): {}",
                e.code, e.message
            )));
        }
        let result = resp.result.ok_or_else(|| {
            sisyphus_tools_core::ToolError::TodoError("list_prompts: missing result".to_string())
        })?;
        let parsed: ListPromptsResult = serde_json::from_value(result).map_err(|e| {
            sisyphus_tools_core::ToolError::TodoError(format!("deserialize prompts: {e}"))
        })?;
        Ok(parsed)
    }

    /// Get a prompt from the MCP server.
    pub async fn get_prompt(
        &mut self,
        name: &str,
        arguments: Option<std::collections::BTreeMap<String, String>>,
    ) -> ToolResult<GetPromptResult> {
        let params = GetPromptParams {
            name: name.to_string(),
            arguments,
        };
        let resp = self
            .send_request(
                "prompts/get",
                Some(serde_json::to_value(params).map_err(|e| {
                    sisyphus_tools_core::ToolError::TodoError(format!("serialize params: {e}"))
                })?),
            )
            .await?;
        if let Some(e) = resp.error {
            return Err(sisyphus_tools_core::ToolError::TodoError(format!(
                "get_prompt error ({}): {}",
                e.code, e.message
            )));
        }
        let result = resp.result.ok_or_else(|| {
            sisyphus_tools_core::ToolError::TodoError("get_prompt: missing result".to_string())
        })?;
        let parsed: GetPromptResult = serde_json::from_value(result).map_err(|e| {
            sisyphus_tools_core::ToolError::TodoError(format!("deserialize prompt: {e}"))
        })?;
        Ok(parsed)
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
    use async_trait::async_trait;
    use serde_json::json;
    use sisyphus_tools_core::ToolResult;
    use std::sync::{Arc, Mutex};

    /// Mock transport that returns pre-defined responses.
    struct MockTransport {
        responses: Arc<Mutex<Vec<String>>>,
        /// 收到的通知（不消费响应队列）。
        notifications: Arc<Mutex<Vec<String>>>,
    }

    impl MockTransport {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses)),
                notifications: Arc::new(Mutex::new(Vec::new())),
            }
        }

        /// 克隆通知记录的句柄（transport 会被移动进 client，需要在外部观察）。
        fn notifications_arc(&self) -> Arc<Mutex<Vec<String>>> {
            Arc::clone(&self.notifications)
        }
    }

    #[async_trait]
    impl McpTransport for MockTransport {
        async fn request(&mut self, _message: &str) -> ToolResult<String> {
            let mut list = self.responses.lock().unwrap();
            if list.is_empty() {
                Err(sisyphus_tools_core::ToolError::TodoError(
                    "no more mock responses".to_string(),
                ))
            } else {
                Ok(list.remove(0))
            }
        }

        /// 通知只记录，不消费响应队列（与协议一致）。
        async fn notify(&mut self, message: &str) -> ToolResult<()> {
            self.notifications.lock().unwrap().push(message.to_string());
            Ok(())
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
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "tools": { "listChanged": true },
                    "resources": { "subscribe": false, "listChanged": false }
                },
                "serverInfo": { "name": "test-server", "version": "1.0.0" }
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
                    { "name": "echo", "description": "Echo input back", "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } } } },
                    { "name": "add", "description": "Add two numbers", "inputSchema": { "type": "object", "properties": { "a": { "type": "number" }, "b": { "type": "number" } } } }
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
                "isError": false
            }
        })
        .to_string()
    }

    fn mock_initialize_response_for_notification() -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": null, "resources": null },
                "serverInfo": { "name": "test", "version": "0.1" }
            }
        })
        .to_string()
    }

    #[tokio::test]
    async fn test_initialize() {
        let transport = MockTransport::new(vec![mock_initialize_response()]);
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
    async fn test_initialize_sends_notification_without_consuming_response() {
        // 回归：通知是 fire-and-forget。旧实现用 request() 发通知，会一直等
        // 服务器回应（合规 MCP 服务器不会回），实测卡满 120s 才报错。
        let transport = MockTransport::new(vec![mock_initialize_response()]);
        let notifications = transport.notifications_arc();
        let mut client = McpClient::new(Box::new(transport));
        client.initialize("test", "0.1").await.unwrap();

        let sent = notifications.lock().unwrap().clone();
        assert_eq!(sent.len(), 1, "应发出 1 条通知: {sent:?}");
        let msg: serde_json::Value = serde_json::from_str(&sent[0]).expect("通知应为 JSON");
        assert_eq!(msg["method"], "notifications/initialized");
        assert!(msg.get("id").is_none(), "通知不应带 id: {sent:?}");
    }

    #[tokio::test]
    async fn test_server_info() {
        let transport = MockTransport::new(vec![mock_initialize_response()]);
        let mut client = McpClient::new(Box::new(transport));
        assert!(client.server_info().is_none());
        client.initialize("test", "0.1").await.unwrap();
        assert_eq!(client.server_info().unwrap().name, "test-server");
    }

    #[tokio::test]
    async fn test_list_resources() {
        let transport = MockTransport::new(vec![
            mock_initialize_response_for_notification(),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {
                    "resources": [
                        { "uri": "memo://a", "name": "a", "description": "A", "mimeType": "text/plain" },
                        { "uri": "memo://b", "name": "b" }
                    ]
                }
            })
            .to_string(),
        ]);
        let mut client = McpClient::new(Box::new(transport));
        client.initialize("test", "0.1").await.unwrap();
        let result = client.list_resources().await.unwrap();
        assert_eq!(result.resources.len(), 2);
        assert_eq!(result.resources[0].uri, "memo://a");
        assert_eq!(result.resources[0].name, "a");
        assert!(result.resource_templates.is_empty());
    }

    #[tokio::test]
    async fn test_read_resource() {
        let transport = MockTransport::new(vec![
            mock_initialize_response_for_notification(),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {
                    "contents": [
                        { "uri": "memo://a", "mimeType": "text/plain", "text": "hello" }
                    ]
                }
            })
            .to_string(),
        ]);
        let mut client = McpClient::new(Box::new(transport));
        client.initialize("test", "0.1").await.unwrap();
        let result = client.read_resource("memo://a").await.unwrap();
        assert_eq!(result.contents.len(), 1);
        assert_eq!(result.contents[0].uri, "memo://a");
        assert_eq!(result.contents[0].text.as_deref(), Some("hello"));
    }

    #[tokio::test]
    async fn test_list_prompts() {
        let transport = MockTransport::new(vec![
            mock_initialize_response_for_notification(),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {
                    "prompts": [
                        {
                            "name": "greet",
                            "description": "hi",
                            "arguments": [{ "name": "name", "required": true }]
                        },
                        { "name": "plain" }
                    ]
                }
            })
            .to_string(),
        ]);
        let mut client = McpClient::new(Box::new(transport));
        client.initialize("test", "0.1").await.unwrap();
        let result = client.list_prompts().await.unwrap();
        assert_eq!(result.prompts.len(), 2);
        assert_eq!(result.prompts[0].name, "greet");
        assert_eq!(result.prompts[0].arguments.len(), 1);
    }

    #[tokio::test]
    async fn test_get_prompt() {
        let transport = MockTransport::new(vec![
            mock_initialize_response_for_notification(),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {
                    "description": "greeting",
                    "messages": [
                        { "role": "user", "content": { "type": "text", "text": "Say hi" } }
                    ]
                }
            })
            .to_string(),
        ]);
        let mut client = McpClient::new(Box::new(transport));
        client.initialize("test", "0.1").await.unwrap();
        let mut args = std::collections::BTreeMap::new();
        args.insert("name".to_string(), "Sisyphus".to_string());
        let result = client.get_prompt("greet", Some(args)).await.unwrap();
        assert_eq!(result.description.as_deref(), Some("greeting"));
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, "user");
    }
}
