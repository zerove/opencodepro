//! Integration tests for sisyphus-tools-mcp ToolRegistry.
//!
//! Exercises the public thread-safe tool metadata registry as an external
//! consumer would: register / lookup / list / filter by prefix. The
//! internal mod tests cover the same API but in-tree; these tests
//! exercise the API exactly as a downstream crate would.

use serde_json::json;
use sisyphus_tools_mcp::tool_registry::{ToolInfo, ToolRegistry, ToolSource};

fn mk_info(name: &str, source: ToolSource) -> ToolInfo {
    ToolInfo {
        name: name.to_string(),
        description: format!("test tool {name}"),
        parameters: Some(json!({"type": "object"})),
        source,
    }
}

#[test]
fn new_registry_is_empty() {
    let reg = ToolRegistry::new();
    assert_eq!(reg.len(), 0);
    assert!(reg.is_empty());
    assert!(reg.list().is_empty());
    assert!(reg.get("anything").is_none());
}

#[test]
fn default_equals_new() {
    // `Default` is documented to delegate to `new`.
    let reg: ToolRegistry = Default::default();
    assert!(reg.is_empty());
}

#[test]
fn register_then_get_round_trip() {
    let reg = ToolRegistry::new();
    let info = mk_info("read", ToolSource::Builtin);
    reg.register("builtin/read", info);

    let got = reg.get("builtin/read").expect("registered");
    assert_eq!(got.name, "read");
    assert_eq!(got.source, ToolSource::Builtin);
    assert!(reg.get("nonexistent").is_none());
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
}

#[test]
fn register_overwrites_existing() {
    let reg = ToolRegistry::new();
    reg.register("id", mk_info("v1", ToolSource::Builtin));
    reg.register("id", mk_info("v2", ToolSource::Strategic));

    let got = reg.get("id").unwrap();
    assert_eq!(got.name, "v2", "second register should overwrite first");
    assert_eq!(got.source, ToolSource::Strategic);
    // len is still 1, no duplicate entry.
    assert_eq!(reg.len(), 1);
}

#[test]
fn list_returns_all_tools_sorted_by_id() {
    let reg = ToolRegistry::new();
    reg.register("zeta", mk_info("z", ToolSource::Builtin));
    reg.register("alpha", mk_info("a", ToolSource::Builtin));
    reg.register("mid", mk_info("m", ToolSource::Builtin));

    let list = reg.list();
    let ids: Vec<&str> = list.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(ids, vec!["alpha", "mid", "zeta"]);
}

#[test]
fn list_by_prefix_filters_correctly() {
    let reg = ToolRegistry::new();
    reg.register("tools:read", mk_info("read", ToolSource::Builtin));
    reg.register("tools:write", mk_info("write", ToolSource::Builtin));
    reg.register("other:other", mk_info("other", ToolSource::Builtin));

    let tools = reg.list_by_prefix("tools:");
    assert_eq!(tools.len(), 2);
    assert!(tools.iter().all(|(k, _)| k.starts_with("tools:")));
}

#[test]
fn list_by_prefix_returns_empty_for_no_match() {
    let reg = ToolRegistry::new();
    reg.register("a", mk_info("a", ToolSource::Builtin));
    assert!(reg.list_by_prefix("nope:").is_empty());
}

#[test]
fn list_for_prefixes_unions_results() {
    let reg = ToolRegistry::new();
    reg.register("tools:read", mk_info("r", ToolSource::Builtin));
    reg.register("strategic:plan", mk_info("p", ToolSource::Builtin));
    reg.register("other:x", mk_info("x", ToolSource::Builtin));

    let result = reg.list_for_prefixes(&["tools:".to_string(), "strategic:".to_string()]);
    let ids: Vec<&str> = result.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(ids, vec!["strategic:plan", "tools:read"]);
}

#[test]
fn list_for_prefixes_with_empty_input_returns_empty() {
    let reg = ToolRegistry::new();
    reg.register("tools:read", mk_info("r", ToolSource::Builtin));
    let result = reg.list_for_prefixes(&[]);
    assert!(result.is_empty());
}

#[test]
fn tool_source_equality_holds() {
    // PartialEq is derived on ToolSource, so variants with the same
    // payload must compare equal. Lock this in as a contract.
    assert_eq!(ToolSource::Builtin, ToolSource::Builtin);
    assert_eq!(
        ToolSource::Mcp { server_prefix: "x".into() },
        ToolSource::Mcp { server_prefix: "x".into() }
    );
    assert_ne!(ToolSource::Builtin, ToolSource::Strategic);
    assert_ne!(ToolSource::Wasm, ToolSource::Strategic);
}

#[test]
fn concurrent_register_does_not_lose_tools() {
    use std::sync::Arc;
    use std::thread;

    let reg = Arc::new(ToolRegistry::new());
    let threads = 4;
    let per_thread = 100;
    let handles: Vec<_> = (0..threads)
        .map(|t| {
            let reg = Arc::clone(&reg);
            thread::spawn(move || {
                for i in 0..per_thread {
                    let id = format!("t{t}-i{i}");
                    reg.register(&id, mk_info(&id, ToolSource::Wasm));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("join");
    }
    assert_eq!(reg.len(), threads * per_thread);
}
