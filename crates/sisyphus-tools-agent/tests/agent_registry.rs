//! Integration tests for sisyphus-tools-agent `agent_registry` module.
//!
//! Exercises the public surfaces of [`AgentRegistry`] and [`AgentInfo`]:
//! registration, lookup, listing, MCP server registration, and the
//! tool-prefix resolution path. Uses [`FixedResponseClient`] as a
//! trivial `LlmClient` implementation.

use sisyphus_tools_agent::agent_registry::{AgentInfo, AgentRegistry};
use sisyphus_tools_llm::FixedResponseClient;
use std::sync::Arc;

fn make_registry() -> AgentRegistry {
    let client: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(FixedResponseClient("x".into()));
    AgentRegistry::new(client)
}

fn info(id: &str, prefixes: Vec<&str>) -> AgentInfo {
    AgentInfo {
        display_name: id.to_string(),
        description: format!("{id} agent"),
        tool_prefixes: prefixes.into_iter().map(String::from).collect(),
        max_concurrency: 4,
    }
}

// ---------------------------------------------------------------------------
// register / get / has / unregister
// ---------------------------------------------------------------------------

#[test]
fn registry_new_is_empty() {
    let r = make_registry();
    assert!(!r.has("any"));
    assert!(r.list().is_empty());
}

#[test]
fn registry_register_and_get() {
    let r = make_registry();
    r.register("alpha", info("alpha", vec!["a/"]));
    assert!(r.has("alpha"));
    let got = r.get("alpha").unwrap();
    assert_eq!(got.display_name, "alpha");
    assert_eq!(got.tool_prefixes, vec!["a/"]);
    assert_eq!(got.max_concurrency, 4);
}

#[test]
fn registry_get_missing_returns_none() {
    let r = make_registry();
    assert!(r.get("nonexistent").is_none());
}

#[test]
fn registry_unregister() {
    let r = make_registry();
    r.register("temp", info("temp", vec![]));
    assert!(r.has("temp"));
    let removed = r.unregister("temp");
    assert!(removed);
    assert!(!r.has("temp"));
}

#[test]
fn registry_unregister_missing_returns_false() {
    let r = make_registry();
    assert!(!r.unregister("never_was"));
}

#[test]
fn registry_register_overwrites() {
    let r = make_registry();
    r.register("a", info("a", vec!["old/"]));
    r.register("a", info("a", vec!["new/"]));
    let got = r.get("a").unwrap();
    assert_eq!(got.tool_prefixes, vec!["new/"]);
}

#[test]
fn registry_list_returns_all() {
    let r = make_registry();
    r.register("a", info("a", vec![]));
    r.register("b", info("b", vec![]));
    r.register("c", info("c", vec![]));
    let list = r.list();
    assert_eq!(list.len(), 3);
    let ids: Vec<_> = list.iter().map(|(k, _)| k.clone()).collect();
    assert!(ids.contains(&"a".to_string()));
    assert!(ids.contains(&"b".to_string()));
    assert!(ids.contains(&"c".to_string()));
}

// ---------------------------------------------------------------------------
// MCP server registration
// ---------------------------------------------------------------------------

#[test]
fn registry_register_mcp_server() {
    let r = make_registry();
    r.register_mcp_server("mcp_a", "server-a", 5);
    // After registering, the prefix is exposed via resolve_agent_for_tool.
    // MCP prefixes are stored as "mcp_a:" (colon separator).
    let resolved = r.resolve_agent_for_tool("mcp_a:tool1");
    assert!(resolved.is_some());
    assert!(resolved.unwrap().starts_with("mcp_"));
}

#[test]
fn registry_resolve_unknown_prefix() {
    let r = make_registry();
    assert!(r.resolve_agent_for_tool("nonexistent__tool").is_none());
}

#[test]
fn registry_resolve_no_mcp_returns_none_for_non_mcp_tool() {
    let r = make_registry();
    r.register("native", info("native", vec!["n/"]));
    // "n/foo" matches the native prefix → resolves to "native".
    let r1 = r.resolve_agent_for_tool("n/foo");
    assert_eq!(r1.as_deref(), Some("native"));
    // "x/foo" doesn't match anything.
    assert!(r.resolve_agent_for_tool("x/foo").is_none());
}

// ---------------------------------------------------------------------------
// register_builtin / builtin_agents
// ---------------------------------------------------------------------------

#[test]
fn registry_builtin_agents_static_list_non_empty() {
    let list = AgentRegistry::builtin_agents();
    // There must be at least one built-in.
    assert!(!list.is_empty());
    for (id, info) in &list {
        assert!(!id.is_empty());
        assert!(!info.display_name.is_empty());
    }
}

#[test]
fn registry_register_builtin_populates() {
    let r = make_registry();
    r.register_builtin();
    // After register_builtin, list must be non-empty.
    let list = r.list();
    assert!(!list.is_empty());
}
