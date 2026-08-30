//! Integration tests for sisyphus-types `types` module's
//! pure enums: `ModelRouteStrategy`, `CircuitState`, `WorkMode`, `EvolutionOp`
//! and structs: `CustomToolDefinition`, `NotepadSnapshot`, `TriggerDef`.

use sisyphus_types::types::{
    CircuitState, CustomToolDefinition, EvolutionOp, ModelRouteStrategy, NotepadSnapshot,
    SisyphusNotepad, TriggerDef, WorkMode,
};

// ---------------------------------------------------------------------------
// ModelRouteStrategy
// ---------------------------------------------------------------------------

#[test]
fn model_route_strategy_equality() {
    assert_eq!(ModelRouteStrategy::CostOptimized, ModelRouteStrategy::CostOptimized);
    assert_ne!(
        ModelRouteStrategy::CostOptimized,
        ModelRouteStrategy::PerformanceOptimized
    );
    assert_ne!(
        ModelRouteStrategy::PerformanceOptimized,
        ModelRouteStrategy::FallbackChain
    );
}

#[test]
fn model_route_strategy_serde() {
    for v in [
        ModelRouteStrategy::CostOptimized,
        ModelRouteStrategy::PerformanceOptimized,
        ModelRouteStrategy::FallbackChain,
    ] {
        let j = serde_json::to_string(&v).unwrap();
        let back: ModelRouteStrategy = serde_json::from_str(&j).unwrap();
        assert_eq!(back, v);
    }
}

#[test]
fn model_route_strategy_copy() {
    let a = ModelRouteStrategy::CostOptimized;
    let b = a; // Copy
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// CircuitState
// ---------------------------------------------------------------------------

#[test]
fn circuit_state_equality() {
    assert_eq!(CircuitState::Closed, CircuitState::Closed);
    assert_eq!(CircuitState::Open, CircuitState::Open);
    assert_eq!(CircuitState::HalfOpen, CircuitState::HalfOpen);
    assert_ne!(CircuitState::Closed, CircuitState::Open);
    assert_ne!(CircuitState::Open, CircuitState::HalfOpen);
    assert_ne!(CircuitState::Closed, CircuitState::HalfOpen);
}

#[test]
fn circuit_state_serde() {
    for s in [CircuitState::Closed, CircuitState::Open, CircuitState::HalfOpen] {
        let j = serde_json::to_string(&s).unwrap();
        let back: CircuitState = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }
}

#[test]
fn circuit_state_copy() {
    let s = CircuitState::HalfOpen;
    let _s2 = s;
    let _s3 = s;
    // If Copy is not implemented, this wouldn't compile.
}

// ---------------------------------------------------------------------------
// WorkMode
// ---------------------------------------------------------------------------

#[test]
fn work_mode_default_is_standard() {
    let m: WorkMode = Default::default();
    assert_eq!(m, WorkMode::Standard);
}

#[test]
fn work_mode_equality_and_serde() {
    assert_eq!(WorkMode::Standard, WorkMode::Standard);
    assert_ne!(WorkMode::Standard, WorkMode::UltraWork);
    let j = serde_json::to_string(&WorkMode::UltraWork).unwrap();
    let back: WorkMode = serde_json::from_str(&j).unwrap();
    assert_eq!(back, WorkMode::UltraWork);
}

// ---------------------------------------------------------------------------
// EvolutionOp
// ---------------------------------------------------------------------------

#[test]
fn evolution_op_adjust_fuel() {
    let op = EvolutionOp::AdjustFuel {
        node_name: "n1".into(),
        delta: 100,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "n1");
            assert_eq!(delta, 100);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn evolution_op_add_retry() {
    let op = EvolutionOp::AddRetry {
        node_name: "n".into(),
        max_retries: 3,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AddRetry {
            node_name,
            max_retries,
        } => {
            assert_eq!(node_name, "n");
            assert_eq!(max_retries, 3);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn evolution_op_insert_node() {
    let op = EvolutionOp::InsertNode {
        after_node: "a".into(),
        new_name: "b".into(),
        tool_id: "t".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::InsertNode {
            after_node,
            new_name,
            tool_id,
        } => {
            assert_eq!(after_node, "a");
            assert_eq!(new_name, "b");
            assert_eq!(tool_id, "t");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn evolution_op_remove_node() {
    let op = EvolutionOp::RemoveNode {
        node_name: "n".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::RemoveNode { node_name } => assert_eq!(node_name, "n"),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn evolution_op_swap_nodes() {
    let op = EvolutionOp::SwapNodes {
        node_a: "a".into(),
        node_b: "b".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::SwapNodes { node_a, node_b } => {
            assert_eq!(node_a, "a");
            assert_eq!(node_b, "b");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn evolution_op_variants_equality_distinct() {
    let ops = vec![
        EvolutionOp::AdjustFuel {
            node_name: "x".into(),
            delta: 1,
        },
        EvolutionOp::AddRetry {
            node_name: "x".into(),
            max_retries: 1,
        },
        EvolutionOp::InsertNode {
            after_node: "x".into(),
            new_name: "y".into(),
            tool_id: "t".into(),
        },
        EvolutionOp::RemoveNode {
            node_name: "x".into(),
        },
        EvolutionOp::SwapNodes {
            node_a: "a".into(),
            node_b: "b".into(),
        },
    ];
    // All should be PartialEq-distinct.
    for (i, a) in ops.iter().enumerate() {
        for (j, b) in ops.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CustomToolDefinition
// ---------------------------------------------------------------------------

#[test]
fn custom_tool_default_fuel_limit() {
    let c: CustomToolDefinition = serde_json::from_value(serde_json::json!({
        "name": "tool1",
        "description": "Test",
        "handler": "/tmp/tool.wasm"
    }))
    .unwrap();
    assert_eq!(c.fuel_limit, 1_000_000);
    assert_eq!(c.name, "tool1");
    assert!(c.schema.is_null());
}

#[test]
fn custom_tool_with_explicit_fuel() {
    let c: CustomToolDefinition = serde_json::from_value(serde_json::json!({
        "name": "t",
        "description": "d",
        "handler": "/tmp/t.wasm",
        "schema": {"type": "object"},
        "fuel_limit": 5000
    }))
    .unwrap();
    assert_eq!(c.fuel_limit, 5000);
    assert!(!c.schema.is_null());
}

#[test]
fn custom_tool_round_trip() {
    let c = CustomToolDefinition {
        name: "x".into(),
        description: "y".into(),
        handler: std::path::PathBuf::from("/tmp/x.wasm"),
        schema: serde_json::json!({"type": "object"}),
        fuel_limit: 1000,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: CustomToolDefinition = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "x");
    assert_eq!(back.fuel_limit, 1000);
    assert_eq!(back.handler.to_str().unwrap(), "/tmp/x.wasm");
}

// ---------------------------------------------------------------------------
// NotepadSnapshot
// ---------------------------------------------------------------------------

#[test]
fn notepad_snapshot_field_access() {
    let s = NotepadSnapshot {
        timestamp: 1234,
        snapshot: im::HashMap::new(),
        trigger_node: "n1".into(),
    };
    assert_eq!(s.timestamp, 1234);
    assert_eq!(s.trigger_node, "n1");
    assert!(s.snapshot.is_empty());
}

// ---------------------------------------------------------------------------
// SisyphusNotepad
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_default_is_empty() {
    let n = SisyphusNotepad::default();
    assert!(n.sections.is_empty());
    assert!(n.history.is_empty());
}

#[test]
fn sisyphus_notepad_write_section() {
    let mut n = SisyphusNotepad::default();
    n.write_section("plan".into(), "do X".into());
    assert_eq!(n.read_section("plan"), Some(&"do X".to_string()));
}

#[test]
fn sisyphus_notepad_read_missing_returns_none() {
    let n = SisyphusNotepad::default();
    assert_eq!(n.read_section("nope"), None);
}

#[test]
fn sisyphus_notepad_append_insight_creates_section() {
    let mut n = SisyphusNotepad::default();
    n.append_insight("scratch".into(), "first idea".into());
    let content = n.read_section("scratch").unwrap();
    assert!(content.contains("first idea"));
}

#[test]
fn sisyphus_notepad_append_insight_concatenates() {
    let mut n = SisyphusNotepad::default();
    n.append_insight("s".into(), "a".into());
    n.append_insight("s".into(), "b".into());
    let c = n.read_section("s").unwrap();
    assert!(c.contains("a"));
    assert!(c.contains("b"));
}

#[test]
fn sisyphus_notepad_take_snapshot() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a".into(), "content".into());
    n.take_snapshot("trigger".into());
    assert_eq!(n.history.len(), 1);
    assert_eq!(n.history[0].trigger_node, "trigger");
    assert!(n.history[0].snapshot.contains_key("a"));
}

#[test]
fn sisyphus_notepad_prune_truncates_long_section() {
    let mut n = SisyphusNotepad::default();
    let long = (0..20).map(|i| format!("line{}", i)).collect::<Vec<_>>().join("\n");
    n.write_section("s".into(), long);
    n.prune(5);
    let c = n.read_section("s").unwrap();
    let lines: Vec<&str> = c.lines().collect();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], "line15");
    assert_eq!(lines[4], "line19");
}

#[test]
fn sisyphus_notepad_prune_no_op_when_under_limit() {
    let mut n = SisyphusNotepad::default();
    n.write_section("s".into(), "a\nb\nc".into());
    n.prune(10);
    assert_eq!(n.read_section("s").unwrap(), "a\nb\nc");
}

// ---------------------------------------------------------------------------
// TriggerDef
// ---------------------------------------------------------------------------

#[test]
fn trigger_def_minimal_serde() {
    let j = "{}";
    let t: TriggerDef = serde_json::from_str(j).unwrap();
    assert!(t.pattern.is_none());
    assert!(t.tools.is_empty());
    assert!(t.description.is_none());
    assert!(t.hook_phase.is_none());
    assert!(t.hook_effect.is_none());
    assert!(t.hook_message.is_none());
}

#[test]
fn trigger_def_full_serde() {
    let t = TriggerDef {
        pattern: Some("on_save".into()),
        tools: vec!["file_edit".into(), "bash".into()],
        description: Some("Runs after save".into()),
        hook_phase: Some("after".into()),
        hook_effect: Some("log".into()),
        hook_message: Some("user saved".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.pattern.as_deref(), Some("on_save"));
    assert_eq!(back.tools, vec!["file_edit", "bash"]);
    assert_eq!(back.hook_phase.as_deref(), Some("after"));
}

#[test]
fn trigger_def_with_only_pattern() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"foo"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("foo"));
    assert!(t.tools.is_empty());
    assert!(t.description.is_none());
}

// ---------------------------------------------------------------------------
// Additional CustomToolDefinition and EvolutionOp coverage
// ---------------------------------------------------------------------------

#[test]
fn custom_tool_with_unicode_name() {
    use sisyphus_types::types::CustomToolDefinition;
    let t = CustomToolDefinition {
        name: "工具-α".into(),
        description: "中文描述".into(),
        handler: std::path::PathBuf::from("/p.wasm"),
        schema: serde_json::json!({}),
        fuel_limit: 1000,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: CustomToolDefinition = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "工具-α");
    assert_eq!(back.description, "中文描述");
}

#[test]
fn custom_tool_with_max_fuel_limit() {
    use sisyphus_types::types::CustomToolDefinition;
    let t = CustomToolDefinition {
        name: "n".into(),
        description: "d".into(),
        handler: std::path::PathBuf::from("/p.wasm"),
        schema: serde_json::json!({}),
        fuel_limit: u64::MAX,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: CustomToolDefinition = serde_json::from_str(&j).unwrap();
    assert_eq!(back.fuel_limit, u64::MAX);
}

#[test]
fn custom_tool_with_complex_schema() {
    use sisyphus_types::types::CustomToolDefinition;
    let t = CustomToolDefinition {
        name: "n".into(),
        description: "d".into(),
        handler: std::path::PathBuf::from("/p.wasm"),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name"]
        }),
        fuel_limit: 1000,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: CustomToolDefinition = serde_json::from_str(&j).unwrap();
    assert_eq!(back.schema["properties"]["name"]["type"], "string");
}

#[test]
fn evolution_op_adjust_fuel_negative_delta() {
    let op = EvolutionOp::AdjustFuel {
        node_name: "n".into(),
        delta: -1000,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "n");
            assert_eq!(delta, -1000);
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn evolution_op_add_retry_max_retries() {
    let op = EvolutionOp::AddRetry {
        node_name: "n".into(),
        max_retries: u32::MAX,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AddRetry { node_name, max_retries } => {
            assert_eq!(node_name, "n");
            assert_eq!(max_retries, u32::MAX);
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn evolution_op_insert_node_serde() {
    let op = EvolutionOp::InsertNode {
        after_node: "n1".into(),
        new_name: "new_node".into(),
        tool_id: "t1".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::InsertNode { after_node, new_name, tool_id } => {
            assert_eq!(after_node, "n1");
            assert_eq!(new_name, "new_node");
            assert_eq!(tool_id, "t1");
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn evolution_op_remove_node_serde() {
    let op = EvolutionOp::RemoveNode { node_name: "n".into() };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::RemoveNode { node_name } => assert_eq!(node_name, "n"),
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn evolution_op_swap_nodes_serde() {
    let op = EvolutionOp::SwapNodes {
        node_a: "a".into(),
        node_b: "b".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::SwapNodes { node_a, node_b } => {
            assert_eq!(node_a, "a");
            assert_eq!(node_b, "b");
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn work_mode_default() {
    let wm = WorkMode::default();
    assert_eq!(wm, WorkMode::Standard);
}

#[test]
fn work_mode_debug() {
    let _ = format!("{:?}", WorkMode::Standard);
    let _ = format!("{:?}", WorkMode::UltraWork);
}

#[test]
fn circuit_state_debug() {
    let _ = format!("{:?}", CircuitState::Closed);
}

// ---------------------------------------------------------------------------
// Additional TriggerDef coverage
// ---------------------------------------------------------------------------

#[test]
fn trigger_def_with_empty_tools() {
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools: vec![],
        description: None,
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert!(back.tools.is_empty());
    assert_eq!(back.pattern.as_deref(), Some("p"));
}

#[test]
fn trigger_def_with_unicode_pattern() {
    let t = TriggerDef {
        pattern: Some("模式-α".into()),
        tools: vec!["工具".into()],
        description: Some("中文描述".into()),
        hook_phase: Some("前".into()),
        hook_effect: Some("执行".into()),
        hook_message: Some("消息".into()),
    };
    assert_eq!(t.pattern.as_deref(), Some("模式-α"));
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description.as_deref(), Some("中文描述"));
}

#[test]
fn trigger_def_clone() {
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools: vec!["t1".into()],
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let cloned = t.clone();
    assert_eq!(cloned.pattern, t.pattern);
    assert_eq!(cloned.tools, t.tools);
}

#[test]
fn trigger_def_with_minimal_fields() {
    // Only pattern set
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"only"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("only"));
    assert!(t.tools.is_empty());
    assert!(t.description.is_none());
    assert!(t.hook_phase.is_none());
    assert!(t.hook_effect.is_none());
    assert!(t.hook_message.is_none());
}
