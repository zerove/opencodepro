use serde_json::Value;
use std::collections::HashMap;
use std::sync::RwLock;

/// Metadata about a registered tool.
#[derive(Clone, Debug)]
pub struct ToolInfo {
    /// Human-readable name (e.g., "read", "write").
    pub name: String,
    /// Description of what this tool does.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub parameters: Option<Value>,
    /// Where this tool originates from.
    pub source: ToolSource,
}

/// Origin of a registered tool.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolSource {
    /// Built-in tool (file_*, web_*, etc.).
    Builtin,
    /// MCP server tool, with the server's router prefix.
    Mcp { server_prefix: String },
    /// Strategic tool (auto_plan, reflect, etc.).
    Strategic,
    /// WASM sandbox tool.
    Wasm,
}

/// Thread-safe registry of tool metadata.
///
/// Maps `tool_id` (e.g. `"tools:read"`, `"builtin/file_read"`) to
/// tool metadata for discovery and introspection.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, ToolInfo>>,
}

impl ToolRegistry {
    /// Create an empty tool registry.
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    /// Register or update a tool's metadata.
    pub fn register(&self, tool_id: &str, info: ToolInfo) {
        if let Ok(mut map) = self.tools.write() {
            map.insert(tool_id.to_string(), info);
        }
    }

    /// Look up a tool's metadata by tool_id.
    pub fn get(&self, tool_id: &str) -> Option<ToolInfo> {
        let map = self.tools.read().ok()?;
        map.get(tool_id).cloned()
    }

    /// List all registered tools and their metadata, sorted by tool_id.
    pub fn list(&self) -> Vec<(String, ToolInfo)> {
        let map = self.tools.read().unwrap_or_else(|e| e.into_inner());
        let mut items: Vec<_> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));
        items
    }

    /// List all tools whose tool_id starts with the given prefix.
    ///
    /// For example, `list_by_prefix("tools:")` returns all tools from an
    /// MCP server registered with prefix `tools`.
    pub fn list_by_prefix(&self, prefix: &str) -> Vec<(String, ToolInfo)> {
        let map = self.tools.read().unwrap_or_else(|e| e.into_inner());
        let mut items: Vec<_> = map
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));
        items
    }

    /// List tools matching a given agent's tool_prefixes.
    ///
    /// Returns all tools whose tool_id starts with any of the given prefixes.
    pub fn list_for_prefixes(&self, prefixes: &[String]) -> Vec<(String, ToolInfo)> {
        let map = self.tools.read().unwrap_or_else(|e| e.into_inner());
        let mut items: Vec<_> = map
            .iter()
            .filter(|(k, _)| prefixes.iter().any(|p| k.starts_with(p)))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));
        items
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.read().map(|m| m.len()).unwrap_or(0)
    }

    /// Check if the registry has any tools.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry() -> ToolRegistry {
        ToolRegistry::new()
    }

    #[test]
    fn test_empty_registry() {
        let reg = make_registry();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.list().is_empty());
    }

    #[test]
    fn test_register_and_get() {
        let reg = make_registry();
        reg.register(
            "tools:echo",
            ToolInfo {
                name: "echo".into(),
                description: "Echo back input".into(),
                parameters: Some(serde_json::json!({
                    "type": "object",
                    "properties": { "text": { "type": "string" } }
                })),
                source: ToolSource::Mcp {
                    server_prefix: "tools".into(),
                },
            },
        );
        assert_eq!(reg.len(), 1);
        let info = reg.get("tools:echo").unwrap();
        assert_eq!(info.name, "echo");
        assert_eq!(info.description, "Echo back input");
        assert!(matches!(info.source, ToolSource::Mcp { .. }));
    }

    #[test]
    fn test_list_by_prefix() {
        let reg = make_registry();
        reg.register(
            "tools:echo",
            ToolInfo {
                name: "echo".into(),
                description: "Echo".into(),
                parameters: None,
                source: ToolSource::Mcp {
                    server_prefix: "tools".into(),
                },
            },
        );
        reg.register(
            "tools:read",
            ToolInfo {
                name: "read".into(),
                description: "Read file".into(),
                parameters: None,
                source: ToolSource::Mcp {
                    server_prefix: "tools".into(),
                },
            },
        );
        reg.register(
            "librarian/research",
            ToolInfo {
                name: "research".into(),
                description: "Web research".into(),
                parameters: None,
                source: ToolSource::Builtin,
            },
        );

        let tools = reg.list_by_prefix("tools:");
        assert_eq!(tools.len(), 2);

        let builtin = reg.list_by_prefix("librarian/");
        assert_eq!(builtin.len(), 1);
    }

    #[test]
    fn test_list_for_prefixes() {
        let reg = make_registry();
        reg.register(
            "tools:echo",
            ToolInfo {
                name: "echo".into(),
                description: "Echo".into(),
                parameters: None,
                source: ToolSource::Mcp {
                    server_prefix: "tools".into(),
                },
            },
        );
        reg.register(
            "files:read",
            ToolInfo {
                name: "read".into(),
                description: "Read".into(),
                parameters: None,
                source: ToolSource::Mcp {
                    server_prefix: "files".into(),
                },
            },
        );

        let tools = reg.list_for_prefixes(&["tools:".into(), "files:".into()]);
        assert_eq!(tools.len(), 2);

        let tools = reg.list_for_prefixes(&["files:".into()]);
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn test_get_nonexistent() {
        let reg = make_registry();
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_tool_info_clone() {
        let info = ToolInfo {
            name: "test".into(),
            description: "desc".into(),
            parameters: None,
            source: ToolSource::Builtin,
        };
        let cloned = info.clone();
        assert_eq!(cloned.name, "test");
    }

    #[test]
    fn test_tool_source_mcp_matches() {
        let a = ToolSource::Mcp {
            server_prefix: "tools".into(),
        };
        let b = ToolSource::Mcp {
            server_prefix: "tools".into(),
        };
        let c = ToolSource::Mcp {
            server_prefix: "files".into(),
        };
        assert!(a == b);
        assert!(a != c);
    }
}
