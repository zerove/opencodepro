//! Sisyphus WASM SDK - Tool development interface for WASM-based SOP nodes.
//!
//! This crate provides the types and registration mechanism for tools that
//! run inside the Sisyphus WASM sandbox. Tool authors implement [`SisyphusTool`]
//! and register their tools via [`register_tool`] so the runtime can discover
//! and execute them.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// ── Types ────────────────────────────────────────────────────────────────

/// Execution context passed to a tool when it is invoked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    pub tool_id: String,
    pub input: Value,
    pub state: Value,
}

/// Output produced by a tool after execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub result: Value,
    pub state_update: Option<Value>,
}

impl ToolOutput {
    /// Convenience constructor for a JSON result with no state update.
    pub fn json(val: Value) -> Self {
        Self {
            result: val,
            state_update: None,
        }
    }
}

// ── Tool trait ────────────────────────────────────────────────────────────

/// Trait that every WASM tool must implement.
///
/// Tools are stateless executors: they receive a [`ToolContext`] and return a
/// [`ToolOutput`].  Any mutable state should be communicated through
/// `ToolContext.state` / `ToolOutput.state_update`.
pub trait SisyphusTool: Send + Sync {
    fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String>;
}

// ── Global tool registry ─────────────────────────────────────────────────

/// Thread-safe global registry that stores tool instances keyed by tool id.
///
/// The registry is initialised lazily on first access and lives for the
/// lifetime of the process (intentionally — WASM tools are loaded once and
/// reused).
struct ToolRegistryInner {
    tools: HashMap<String, Arc<dyn SisyphusTool>>,
}

static REGISTRY: OnceLock<Mutex<ToolRegistryInner>> = OnceLock::new();

fn registry() -> &'static Mutex<ToolRegistryInner> {
    REGISTRY.get_or_init(|| {
        Mutex::new(ToolRegistryInner {
            tools: HashMap::new(),
        })
    })
}

/// Register a tool in the global registry.
///
/// The tool is stored under its type name (`std::any::type_name::<T>()`).
/// If a tool with the same id already exists it is silently replaced.
pub fn register_tool<T: SisyphusTool + 'static>(tool: T) {
    let id = std::any::type_name::<T>().to_string();
    let Some(mut reg) = registry().lock().ok() else {
        tracing::error!("tool registry poisoned, register_tool ignored");
        return;
    };
    reg.tools.insert(id, Arc::new(tool));
}

/// Register a tool with an explicit id.
pub fn register_tool_as<T: SisyphusTool + 'static>(tool_id: &str, tool: T) {
    let Some(mut reg) = registry().lock().ok() else {
        tracing::error!("tool registry poisoned, register_tool_as ignored");
        return;
    };
    reg.tools.insert(tool_id.to_string(), Arc::new(tool));
}

/// Retrieve a tool reference by id.
pub fn get_tool(tool_id: &str) -> Option<Arc<dyn SisyphusTool>> {
    let reg = registry().lock().ok()?;
    reg.tools.get(tool_id).cloned()
}

/// List all registered tool ids.
pub fn list_tools() -> Vec<String> {
    registry()
        .lock()
        .ok()
        .map(|reg| reg.tools.keys().cloned().collect())
        .unwrap_or_default()
}

/// Number of registered tools.
pub fn tool_count() -> usize {
    registry()
        .lock()
        .ok()
        .map(|reg| reg.tools.len())
        .unwrap_or_default()
}

/// Remove a tool from the registry by id. Returns `true` if it existed.
pub fn remove_tool(tool_id: &str) -> bool {
    registry()
        .lock()
        .ok()
        .map(|mut reg| reg.tools.remove(tool_id).is_some())
        .unwrap_or(false)
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoTool;
    impl SisyphusTool for EchoTool {
        fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String> {
            Ok(ToolOutput::json(ctx.input.clone()))
        }
    }

    struct FailingTool;
    impl SisyphusTool for FailingTool {
        fn execute(&self, _ctx: &ToolContext) -> Result<ToolOutput, String> {
            Err("intentional failure".into())
        }
    }

    #[test]
    fn test_register_and_list() {
        // Clear registry for test isolation
        {
            let mut reg = registry().lock().unwrap();
            reg.tools.clear();
        }

        register_tool_as("echo", EchoTool);
        let tools = list_tools();
        assert!(
            tools.contains(&"echo".to_string()),
            "echo should be registered"
        );
    }

    #[test]
    fn test_tool_count() {
        {
            let mut reg = registry().lock().unwrap();
            reg.tools.clear();
        }

        assert_eq!(tool_count(), 0);
        register_tool_as("count_a", EchoTool);
        register_tool_as("count_b", FailingTool);
        assert_eq!(tool_count(), 2);
    }

    #[test]
    fn test_remove_tool() {
        {
            let mut reg = registry().lock().unwrap();
            reg.tools.clear();
        }

        register_tool_as("removable", EchoTool);
        assert!(remove_tool("removable"));
        assert!(!remove_tool("removable")); // already removed
        assert_eq!(tool_count(), 0);
    }

    #[test]
    fn test_duplicate_registration_replaces() {
        {
            let mut reg = registry().lock().unwrap();
            reg.tools.clear();
        }

        register_tool_as("dup", EchoTool);
        register_tool_as("dup", FailingTool);
        assert_eq!(tool_count(), 1);

        // The second registration should have replaced the first
        let tools = list_tools();
        assert!(tools.contains(&"dup".to_string()));
    }

    #[test]
    fn test_tool_output_json() {
        let output = ToolOutput::json(serde_json::json!({"status": "ok"}));
        assert_eq!(output.result, serde_json::json!({"status": "ok"}));
        assert!(output.state_update.is_none());
    }

    #[test]
    fn test_tool_output_with_state() {
        let output = ToolOutput {
            result: serde_json::json!("done"),
            state_update: Some(serde_json::json!({"counter": 1})),
        };
        assert!(output.state_update.is_some());
    }

    #[test]
    fn test_get_tool_returns_arc() {
        {
            let mut reg = registry().lock().unwrap();
            reg.tools.clear();
        }

        register_tool_as("arc_test", EchoTool);
        let tool = get_tool("arc_test");
        assert!(tool.is_some(), "get_tool should find registered tool");
        let tool = tool.unwrap();
        let ctx = ToolContext {
            tool_id: "arc_test".into(),
            input: serde_json::json!({"hello": "world"}),
            state: serde_json::json!({}),
        };
        let output = tool.execute(&ctx).unwrap();
        assert_eq!(output.result, serde_json::json!({"hello": "world"}));
    }

    #[test]
    fn test_get_tool_missing() {
        {
            let mut reg = registry().lock().unwrap();
            reg.tools.clear();
        }

        assert!(get_tool("nonexistent").is_none());
    }

    #[test]
    fn test_tool_context_serialization() {
        let ctx = ToolContext {
            tool_id: "test".into(),
            input: serde_json::json!({"key": "value"}),
            state: serde_json::json!({}),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let deserialized: ToolContext = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tool_id, "test");
    }
}
