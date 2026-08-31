//! Integration tests for sisyphus-tools-mcp `tool_registry`.
//!
//! Exercises the public `ToolRegistry` API: register, lookup,
//! list, prefix filtering, and source-based discrimination.

use sisyphus_tools_mcp::tool_registry::{ToolInfo, ToolRegistry, ToolSource};

fn make_info(name: &str, source: ToolSource) -> ToolInfo {
    ToolInfo {
        name: name.into(),
        description: format!("{name} description"),
        parameters: Some(serde_json::json!({"type": "object"})),
        source,
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn new_starts_empty() {
    let reg = ToolRegistry::new();
    assert_eq!(reg.len(), 0);
    assert!(reg.is_empty());
    assert!(reg.list().is_empty());
}

#[test]
fn default_constructor_works() {
    let reg = ToolRegistry::default();
    assert!(reg.is_empty());
}

// ---------------------------------------------------------------------------
// Register / get
// ---------------------------------------------------------------------------

#[test]
fn register_then_get_returns_same_info() {
    let reg = ToolRegistry::new();
    reg.register(
        "tools:echo",
        make_info(
            "echo",
            ToolSource::Mcp {
                server_prefix: "tools".into(),
            },
        ),
    );
    let got = reg.get("tools:echo").expect("present");
    assert_eq!(got.name, "echo");
    assert!(matches!(
        got.source,
        ToolSource::Mcp { ref server_prefix } if server_prefix == "tools"
    ));
}

#[test]
fn get_unknown_id_returns_none() {
    let reg = ToolRegistry::new();
    assert!(reg.get("missing").is_none());
}

#[test]
fn register_overwrites_existing() {
    let reg = ToolRegistry::new();
    reg.register("x", make_info("v1", ToolSource::Builtin));
    reg.register("x", make_info("v2", ToolSource::Builtin));
    assert_eq!(reg.len(), 1);
    let got = reg.get("x").expect("present");
    assert_eq!(got.name, "v2");
}

#[test]
fn register_distinct_ids_all_present() {
    let reg = ToolRegistry::new();
    for i in 0..5 {
        reg.register(
            &format!("id_{i}"),
            make_info(&format!("tool_{i}"), ToolSource::Builtin),
        );
    }
    assert_eq!(reg.len(), 5);
    for i in 0..5 {
        assert!(reg.get(&format!("id_{i}")).is_some());
    }
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

#[test]
fn list_is_sorted_by_id() {
    let reg = ToolRegistry::new();
    reg.register("z_last", make_info("z", ToolSource::Builtin));
    reg.register("a_first", make_info("a", ToolSource::Builtin));
    reg.register("m_mid", make_info("m", ToolSource::Builtin));
    let list = reg.list();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].0, "a_first");
    assert_eq!(list[1].0, "m_mid");
    assert_eq!(list[2].0, "z_last");
}

// ---------------------------------------------------------------------------
// list_by_prefix
// ---------------------------------------------------------------------------

#[test]
fn list_by_prefix_filters_correctly() {
    let reg = ToolRegistry::new();
    reg.register(
        "tools:echo",
        make_info(
            "echo",
            ToolSource::Mcp {
                server_prefix: "tools".into(),
            },
        ),
    );
    reg.register(
        "tools:read",
        make_info(
            "read",
            ToolSource::Mcp {
                server_prefix: "tools".into(),
            },
        ),
    );
    reg.register(
        "librarian/research",
        make_info("research", ToolSource::Builtin),
    );
    assert_eq!(reg.list_by_prefix("tools:").len(), 2);
    assert_eq!(reg.list_by_prefix("librarian/").len(), 1);
}

#[test]
fn list_by_prefix_no_match_is_empty() {
    let reg = ToolRegistry::new();
    reg.register("a", make_info("a", ToolSource::Builtin));
    let list = reg.list_by_prefix("xyz");
    assert!(list.is_empty());
}

#[test]
fn list_by_prefix_returns_sorted_results() {
    let reg = ToolRegistry::new();
    reg.register("p:z", make_info("z", ToolSource::Builtin));
    reg.register("p:a", make_info("a", ToolSource::Builtin));
    reg.register("p:m", make_info("m", ToolSource::Builtin));
    let list = reg.list_by_prefix("p:");
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].0, "p:a");
    assert_eq!(list[2].0, "p:z");
}

// ---------------------------------------------------------------------------
// list_for_prefixes
// ---------------------------------------------------------------------------

#[test]
fn list_for_prefixes_combines() {
    let reg = ToolRegistry::new();
    reg.register("a:x", make_info("x", ToolSource::Builtin));
    reg.register("b:y", make_info("y", ToolSource::Builtin));
    reg.register("c:z", make_info("z", ToolSource::Builtin));
    let list = reg.list_for_prefixes(&["a:".into(), "b:".into()]);
    assert_eq!(list.len(), 2);
}

#[test]
fn list_for_prefixes_empty_input_returns_empty() {
    let reg = ToolRegistry::new();
    reg.register("a:x", make_info("x", ToolSource::Builtin));
    let list = reg.list_for_prefixes(&[]);
    assert!(list.is_empty());
}

#[test]
fn list_for_prefixes_no_matches() {
    let reg = ToolRegistry::new();
    reg.register("a:x", make_info("x", ToolSource::Builtin));
    let list = reg.list_for_prefixes(&["missing:".into()]);
    assert!(list.is_empty());
}

// ---------------------------------------------------------------------------
// ToolSource discrimination
// ---------------------------------------------------------------------------

#[test]
fn tool_source_equality() {
    let a = ToolSource::Mcp {
        server_prefix: "x".into(),
    };
    let b = ToolSource::Mcp {
        server_prefix: "x".into(),
    };
    let c = ToolSource::Mcp {
        server_prefix: "y".into(),
    };
    let d = ToolSource::Builtin;
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_ne!(a, d);
}

#[test]
fn tool_source_strategic_and_wasm_distinct() {
    assert_ne!(ToolSource::Strategic, ToolSource::Wasm);
    assert_ne!(ToolSource::Builtin, ToolSource::Strategic);
    assert_eq!(ToolSource::Builtin, ToolSource::Builtin);
    assert_eq!(ToolSource::Wasm, ToolSource::Wasm);
}
