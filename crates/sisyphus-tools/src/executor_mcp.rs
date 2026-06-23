use std::collections::HashMap;
use serde_json::{json, Value};
use crate::ToolResult;
use crate::mcp::{McpClient, McpTransport, StdioTransport, HttpTransport};

/// A `ToolExecutor` that calls external tools via the MCP protocol.
///
/// Each `execute()` call dispatches to the MCP server's `tools/call` method
/// using `tool_id` as the tool name and `args` as JSON arguments.
pub struct MCPToolExecutor {
    client: tokio::sync::Mutex<McpClient>,
    /// Optional map from tool_id to MCP tool name.
    /// If empty, tool_id is used directly as the MCP tool name.
    tool_map: HashMap<String, String>,
}

impl MCPToolExecutor {
    /// Create an MCPToolExecutor from a stdio command.
    ///
    /// The command is spawned immediately, and the MCP `initialize` handshake
    /// is performed before returning.
    pub async fn from_stdio(command: &str, args: &[&str]) -> ToolResult<Self> {
        let transport = StdioTransport::spawn(command, args).await?;
        Self::from_transport(Box::new(transport)).await
    }

    /// Create an MCPToolExecutor that connects to an HTTP MCP server.
    ///
    /// The `base_url` should be the full URL of the MCP endpoint (e.g.
    /// `http://localhost:3000/mcp`). The `initialize` handshake is performed
    /// before returning.
    pub async fn from_url(base_url: &str) -> ToolResult<Self> {
        let transport = HttpTransport::new(base_url);
        Self::from_transport(Box::new(transport)).await
    }

    /// Create an MCPToolExecutor from an existing transport.
    ///
    /// Runs the MCP `initialize` handshake automatically.
    pub async fn from_transport(transport: Box<dyn McpTransport>) -> ToolResult<Self> {
        let mut client = McpClient::new(transport);
        client.initialize("sisyphus", "0.1.0").await?;
        Ok(Self {
            client: tokio::sync::Mutex::new(client),
            tool_map: HashMap::new(),
        })
    }

    /// Register a mapping from tool_id to MCP tool name.
    /// This allows the executor to route `tool_id` values to differently-named MCP tools.
    pub fn map_tool(&mut self, tool_id: &str, mcp_tool_name: &str) {
        self.tool_map.insert(tool_id.to_string(), mcp_tool_name.to_string());
    }

    /// List tools available on the MCP server.
    pub async fn list_tools(&self) -> ToolResult<Vec<crate::mcp::McpTool>> {
        let mut client = self.client.lock().await;
        client.list_tools().await
    }

    /// Close the underlying transport.
    pub async fn close(&self) -> ToolResult<()> {
        let mut client = self.client.lock().await;
        client.close().await
    }
}

#[async_trait::async_trait]
impl sisyphus_types::ToolExecutor for MCPToolExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, String> {
        // Resolve MCP tool name
        let mcp_tool = self.tool_map
            .get(tool_id)
            .map(|s| s.as_str())
            .unwrap_or(tool_id);

        // Parse optional args
        let mcp_args: Option<Value> = if args.is_empty() {
            None
        } else {
            match serde_json::from_slice(&args) {
                Ok(v) => Some(v),
                Err(e) => return Err(format!("invalid JSON args for MCP tool '{mcp_tool}': {e}")),
            }
        };

        let mut client = self.client.lock().await;
        let result = client.call_tool(mcp_tool, mcp_args).await
            .map_err(|e| format!("MCP tool '{mcp_tool}' failed: {e}"))?;

        // Serialize response
        let output = if result.content.len() == 1 {
            // Single content item — extract directly
            match &result.content[0] {
                crate::mcp::ToolContent::Text { text } => {
                    json!({ "content": text, "is_error": result.is_error.unwrap_or(false) })
                }
                crate::mcp::ToolContent::Resource { resource } => {
                    json!({
                        "content": resource.text,
                        "uri": resource.uri,
                        "mime_type": resource.mime_type,
                        "is_error": result.is_error.unwrap_or(false),
                    })
                }
            }
        } else {
            // Multiple content items — return as array
            let items: Vec<Value> = result.content.iter().map(|c| match c {
                crate::mcp::ToolContent::Text { text } => {
                    json!({ "type": "text", "text": text })
                }
                crate::mcp::ToolContent::Resource { resource } => {
                    json!({
                        "type": "resource",
                        "uri": resource.uri,
                        "text": resource.text,
                        "mime_type": resource.mime_type,
                    })
                }
            }).collect();
            json!({ "content": items, "is_error": result.is_error.unwrap_or(false) })
        };

        serde_json::to_vec(&output)
            .map_err(|e| format!("serialize MCP result: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolResult;
    use async_trait::async_trait;
    use sisyphus_types::ToolExecutor;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    /// A mock transport for testing MCPToolExecutor.
    struct MockMcpTransport {
        responses: Arc<Mutex<Vec<String>>>,
    }

    impl MockMcpTransport {
        fn new(responses: Vec<String>) -> Self {
            Self { responses: Arc::new(Mutex::new(responses)) }
        }
    }

    #[async_trait]
    impl McpTransport for MockMcpTransport {
        async fn request(&mut self, _message: &str) -> ToolResult<String> {
            let mut list = self.responses.lock().unwrap();
            if list.is_empty() {
                Err(crate::ToolError::TodoError("no more mock responses".to_string()))
            } else {
                Ok(list.remove(0))
            }
        }

        async fn close(&mut self) -> ToolResult<()> {
            Ok(())
        }
    }

    fn mock_initialized_responses() -> Vec<String> {
        vec![
            // initialize response
            json!({
                "jsonrpc": "2.0", "id": 1,
                "result": {
                    "protocol_version": "2025-03-26",
                    "capabilities": { "tools": { "list_changed": true }, "resources": null },
                    "server_info": { "name": "mock-mcp", "version": "1.0.0" }
                }
            }).to_string(),
            // initialized notification response (empty)
            "{}".to_string(),
        ]
    }

    #[tokio::test]
    async fn test_mcp_executor_basic_call() {
        let mut responses = mock_initialized_responses();
        responses.push(json!({
            "jsonrpc": "2.0", "id": 3,
            "result": {
                "content": [{ "type": "text", "text": "hello back" }],
                "is_error": false
            }
        }).to_string());

        let transport = MockMcpTransport::new(responses);
        let ex = MCPToolExecutor::from_transport(Box::new(transport)).await.unwrap();
        let args = serde_json::to_vec(&json!({"text": "hello"})).unwrap();
        let result = ex.execute("echo", args, 0).await.unwrap();
        let v: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["content"], "hello back");
        assert_eq!(v["is_error"], false);
    }

    #[tokio::test]
    async fn test_mcp_executor_empty_args() {
        let mut responses = mock_initialized_responses();
        responses.push(json!({
            "jsonrpc": "2.0", "id": 3,
            "result": {
                "content": [{ "type": "text", "text": "done" }],
                "is_error": false
            }
        }).to_string());

        let transport = MockMcpTransport::new(responses);
        let ex = MCPToolExecutor::from_transport(Box::new(transport)).await.unwrap();
        let result = ex.execute("ping", vec![], 0).await.unwrap();
        let v: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["content"], "done");
    }

    #[tokio::test]
    async fn test_mcp_executor_error() {
        let mut responses = mock_initialized_responses();
        responses.push(json!({
            "jsonrpc": "2.0", "id": 3,
            "error": { "code": -32001, "message": "execution failed" }
        }).to_string());

        let transport = MockMcpTransport::new(responses);
        let ex = MCPToolExecutor::from_transport(Box::new(transport)).await.unwrap();
        let result = ex.execute("bad_tool", vec![], 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("execution failed"));
    }

    #[tokio::test]
    async fn test_mcp_executor_tool_map() {
        let mut responses = mock_initialized_responses();
        responses.push(json!({
            "jsonrpc": "2.0", "id": 3,
            "result": {
                "content": [{ "type": "text", "text": "mapped result" }],
                "is_error": false
            }
        }).to_string());

        let transport = MockMcpTransport::new(responses);
        let mut ex = MCPToolExecutor::from_transport(Box::new(transport)).await.unwrap();
        ex.map_tool("my_alias", "real_tool_name");

        let result = ex.execute("my_alias", vec![], 0).await.unwrap();
        let v: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["content"], "mapped result");
    }

    #[tokio::test]
    async fn test_mcp_executor_list_tools() {
        let mut responses = mock_initialized_responses();
        responses.push(json!({
            "jsonrpc": "2.0", "id": 3,
            "result": {
                "tools": [
                    { "name": "greet", "description": "Say hello", "input_schema": { "type": "object" } }
                ]
            }
        }).to_string());

        let transport = MockMcpTransport::new(responses);
        let ex = MCPToolExecutor::from_transport(Box::new(transport)).await.unwrap();
        let tools = ex.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "greet");
    }

    #[tokio::test]
    async fn test_mcp_executor_invalid_json_args() {
        let ex = {
            let transport = MockMcpTransport::new(mock_initialized_responses());
            MCPToolExecutor::from_transport(Box::new(transport)).await.unwrap()
        };
        let result = ex.execute("tool", b"not valid json".to_vec(), 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid JSON"));
    }

    #[tokio::test]
    async fn test_mcp_executor_multi_content() {
        let mut responses = mock_initialized_responses();
        responses.push(json!({
            "jsonrpc": "2.0", "id": 3,
            "result": {
                "content": [
                    { "type": "text", "text": "part1" },
                    { "type": "text", "text": "part2" }
                ],
                "is_error": false
            }
        }).to_string());

        let transport = MockMcpTransport::new(responses);
        let ex = MCPToolExecutor::from_transport(Box::new(transport)).await.unwrap();
        let result = ex.execute("multi", vec![], 0).await.unwrap();
        let v: Value = serde_json::from_slice(&result).unwrap();
        assert!(v["content"].is_array());
        assert_eq!(v["content"].as_array().unwrap().len(), 2);
    }
}
