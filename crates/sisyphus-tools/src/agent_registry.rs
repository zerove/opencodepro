use crate::llm::DynamicLlmClient;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Metadata about a registered agent.
#[derive(Clone, Debug)]
pub struct AgentInfo {
    /// Human-readable name.
    pub display_name: String,
    /// What this agent does.
    pub description: String,
    /// Tool ID prefixes this agent handles (e.g. `"librarian/"`).
    pub tool_prefixes: Vec<String>,
    /// Max parallel tasks for this agent.
    pub max_concurrency: usize,
}

/// Thread-safe registry of available agents.
///
/// Maps agent IDs to their metadata and provides LLM client resolution
/// using the per-agent model overrides from `Config.llm.agents`.
pub struct AgentRegistry {
    agents: RwLock<HashMap<String, AgentInfo>>,
    llm_client: Arc<DynamicLlmClient>,
}

impl AgentRegistry {
    /// Create a new registry with the given shared config.
    ///
    /// Does NOT register built-in agents automatically —
    /// call `register_builtin()` if desired.
    pub fn new(llm_client: Arc<DynamicLlmClient>) -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            llm_client,
        }
    }

    /// Register or update an agent.
    pub fn register(&self, id: &str, info: AgentInfo) {
        let mut map = self.agents.write().unwrap_or_else(|e| e.into_inner());
        map.insert(id.to_string(), info);
    }

    /// Look up an agent by ID.
    pub fn get(&self, id: &str) -> Option<AgentInfo> {
        let map = self.agents.read().unwrap_or_else(|e| e.into_inner());
        map.get(id).cloned()
    }

    /// Check if an agent is registered.
    pub fn has(&self, id: &str) -> bool {
        let map = self.agents.read().unwrap_or_else(|e| e.into_inner());
        map.contains_key(id)
    }

    /// List all registered agent IDs and their info.
    pub fn list(&self) -> Vec<(String, AgentInfo)> {
        let map = self.agents.read().unwrap_or_else(|e| e.into_inner());
        let mut items: Vec<_> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));
        items
    }

    /// Get the LLM params for a given agent, using per-agent overrides.
    ///
    /// Returns `(api_key, endpoint, model)`.
    pub fn get_llm_params(&self, agent_id: &str) -> (String, String, String) {
        self.llm_client.get_params_for(agent_id)
    }

    /// Unregister an agent.
    pub fn unregister(&self, id: &str) -> bool {
        let mut map = self.agents.write().unwrap_or_else(|e| e.into_inner());
        map.remove(id).is_some()
    }

    /// Register the built-in agents with their default tool prefixes.
    pub fn register_builtin(&self) {
        let builtin: Vec<(&str, AgentInfo)> = vec![
            (
                "librarian",
                AgentInfo {
                    display_name: "Librarian".into(),
                    description: "Web research, documentation fetching, code example search".into(),
                    tool_prefixes: vec!["librarian/".into()],
                    max_concurrency: 4,
                },
            ),
            (
                "metis",
                AgentInfo {
                    display_name: "Metis".into(),
                    description: "Requirement analysis, ambiguity detection, plan consulting"
                        .into(),
                    tool_prefixes: vec!["metis/".into()],
                    max_concurrency: 2,
                },
            ),
            (
                "momus",
                AgentInfo {
                    display_name: "Momus".into(),
                    description: "Plan review, code review, architecture review".into(),
                    tool_prefixes: vec!["momus/".into()],
                    max_concurrency: 3,
                },
            ),
            (
                "scribe",
                AgentInfo {
                    display_name: "Scribe".into(),
                    description: "ADR generation, documentation updates".into(),
                    tool_prefixes: vec!["scribe/".into()],
                    max_concurrency: 2,
                },
            ),
            (
                "security",
                AgentInfo {
                    display_name: "Security Auditor".into(),
                    description: "Code audit, threat modeling".into(),
                    tool_prefixes: vec!["security/".into()],
                    max_concurrency: 2,
                },
            ),
            (
                "delegate",
                AgentInfo {
                    display_name: "Delegate".into(),
                    description: "Parallel task decomposition and execution".into(),
                    tool_prefixes: vec!["delegate/".into()],
                    max_concurrency: 8,
                },
            ),
        ];
        for (id, info) in builtin {
            self.register(id, info);
        }
    }

    /// Register an MCP server as an agent with the given tool prefix.
    ///
    /// This makes all tools under the MCP server's prefix discoverable
    /// through the agent registry. The agent ID is `mcp_{prefix}`.
    pub fn register_mcp_server(&self, prefix: &str, server_label: &str, num_tools: usize) {
        let agent_id = format!("mcp_{}", prefix);
        let info = AgentInfo {
            display_name: format!("MCP:{}", server_label),
            description: format!(
                "MCP server '{}' with {} tool(s). Prefix: '{}:'",
                server_label, num_tools, prefix
            ),
            tool_prefixes: vec![format!("{}:", prefix)],
            max_concurrency: 4,
        };
        self.register(&agent_id, info);
    }

    /// Resolve which agent handles a given tool ID by matching prefixes.
    ///
    /// Returns `Some(agent_id)` if a matching agent is found.
    pub fn resolve_agent_for_tool(&self, tool_id: &str) -> Option<String> {
        let map = self.agents.read().unwrap_or_else(|e| e.into_inner());
        for (agent_id, info) in map.iter() {
            for prefix in &info.tool_prefixes {
                if tool_id.starts_with(prefix) {
                    return Some(agent_id.clone());
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::DynamicLlmClient;
    use sisyphus_types::Config;
    use std::sync::Arc;

    fn make_registry() -> AgentRegistry {
        let config = Arc::new(RwLock::new(Config::default()));
        let client = Arc::new(DynamicLlmClient::new(config));
        AgentRegistry::new(client)
    }

    #[test]
    fn test_registry_empty() {
        let reg = make_registry();
        assert!(reg.list().is_empty());
        assert!(!reg.has("metis"));
        assert!(reg.get("metis").is_none());
    }

    #[test]
    fn test_register_and_get() {
        let reg = make_registry();
        reg.register(
            "metis",
            AgentInfo {
                display_name: "Metis".into(),
                description: "Analysis".into(),
                tool_prefixes: vec!["metis/".into()],
                max_concurrency: 2,
            },
        );
        assert!(reg.has("metis"));
        let info = reg.get("metis").unwrap();
        assert_eq!(info.display_name, "Metis");
        assert_eq!(info.tool_prefixes, vec!["metis/"]);
    }

    #[test]
    fn test_register_builtin() {
        let reg = make_registry();
        reg.register_builtin();
        let agents = reg.list();
        assert_eq!(agents.len(), 6, "should have 6 built-in agents");
        assert!(reg.has("librarian"));
        assert!(reg.has("metis"));
        assert!(reg.has("momus"));
        assert!(reg.has("scribe"));
        assert!(reg.has("security"));
        assert!(reg.has("delegate"));
    }

    #[test]
    fn test_unregister() {
        let reg = make_registry();
        reg.register(
            "test",
            AgentInfo {
                display_name: "Test".into(),
                description: "".into(),
                tool_prefixes: vec![],
                max_concurrency: 1,
            },
        );
        assert!(reg.has("test"));
        assert!(reg.unregister("test"));
        assert!(!reg.has("test"));
        assert!(!reg.unregister("test"));
    }

    #[test]
    fn test_resolve_agent_for_tool() {
        let reg = make_registry();
        reg.register_builtin();

        assert_eq!(
            reg.resolve_agent_for_tool("librarian/research"),
            Some("librarian".into())
        );
        assert_eq!(
            reg.resolve_agent_for_tool("metis/analyse"),
            Some("metis".into())
        );
        assert_eq!(
            reg.resolve_agent_for_tool("momus/review_code"),
            Some("momus".into())
        );
        assert_eq!(
            reg.resolve_agent_for_tool("scribe/generate_adr"),
            Some("scribe".into())
        );
        assert_eq!(
            reg.resolve_agent_for_tool("security/audit_code"),
            Some("security".into())
        );
        assert_eq!(
            reg.resolve_agent_for_tool("delegate/run"),
            Some("delegate".into())
        );
    }

    #[test]
    fn test_resolve_unknown_tool() {
        let reg = make_registry();
        reg.register_builtin();
        assert_eq!(reg.resolve_agent_for_tool("file_read"), None);
        assert_eq!(reg.resolve_agent_for_tool("bash_exec"), None);
        assert_eq!(reg.resolve_agent_for_tool("nonexistent/madeup"), None);
    }

    #[test]
    fn test_register_overwrite() {
        let reg = make_registry();
        reg.register(
            "metis",
            AgentInfo {
                display_name: "Old".into(),
                description: "".into(),
                tool_prefixes: vec![],
                max_concurrency: 1,
            },
        );
        reg.register(
            "metis",
            AgentInfo {
                display_name: "New".into(),
                description: "Updated".into(),
                tool_prefixes: vec!["metis/".into()],
                max_concurrency: 4,
            },
        );
        let info = reg.get("metis").unwrap();
        assert_eq!(info.display_name, "New");
        assert_eq!(info.max_concurrency, 4);
    }

    #[test]
    fn test_get_llm_params_default() {
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");
        let reg = make_registry();
        reg.register_builtin();

        let (api, ep, model) = reg.get_llm_params("librarian");
        assert_eq!(api, ""); // default config has empty api_key
        assert_eq!(ep, "http://localhost:8000/v1/chat/completions");
        assert_eq!(model, "gpt-4o");
    }

    #[test]
    fn test_agent_info_clone() {
        let info = AgentInfo {
            display_name: "Test".into(),
            description: "desc".into(),
            tool_prefixes: vec!["test/".into()],
            max_concurrency: 3,
        };
        let cloned = info.clone();
        assert_eq!(cloned.display_name, "Test");
        assert_eq!(cloned.max_concurrency, 3);
    }

    #[test]
    fn test_register_mcp_server() {
        let reg = make_registry();
        reg.register_mcp_server("tools", "file-server", 5);
        let agent_id = "mcp_tools";
        assert!(reg.has(agent_id));
        let info = reg.get(agent_id).unwrap();
        assert_eq!(info.display_name, "MCP:file-server");
        assert!(info.description.contains("5 tool(s)"));
        assert!(info.description.contains("'tools:'"));
        assert_eq!(info.tool_prefixes, vec!["tools:"]);
        assert_eq!(info.max_concurrency, 4);
    }

    #[test]
    fn test_mcp_tool_resolution() {
        let reg = make_registry();
        reg.register_builtin();
        reg.register_mcp_server("tools", "file-server", 3);

        // MCP tools should resolve to the mcp_<prefix> agent
        assert_eq!(
            reg.resolve_agent_for_tool("tools:read"),
            Some("mcp_tools".into())
        );
        assert_eq!(
            reg.resolve_agent_for_tool("tools:write"),
            Some("mcp_tools".into())
        );

        // Built-in tools still resolve correctly
        assert_eq!(
            reg.resolve_agent_for_tool("librarian/research"),
            Some("librarian".into())
        );
    }
}
