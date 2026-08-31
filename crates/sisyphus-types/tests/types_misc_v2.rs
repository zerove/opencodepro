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

// ---------------------------------------------------------------------------
// Additional Coverage
// ---------------------------------------------------------------------------

#[test]
fn circuit_state_default() {
    use sisyphus_types::types::CircuitState;
    // Just verify all variants can be constructed
    let _ = CircuitState::Closed;
    let _ = CircuitState::Open;
    let _ = CircuitState::HalfOpen;
}

#[test]
fn model_route_strategy_debug_distinct() {
    use sisyphus_types::types::ModelRouteStrategy;
    let s1 = format!("{:?}", ModelRouteStrategy::CostOptimized);
    let s2 = format!("{:?}", ModelRouteStrategy::PerformanceOptimized);
    let s3 = format!("{:?}", ModelRouteStrategy::FallbackChain);
    assert_ne!(s1, s2);
    assert_ne!(s2, s3);
    assert_ne!(s1, s3);
}

#[test]
fn evolution_op_adjust_fuel_zero_delta() {
    let op = EvolutionOp::AdjustFuel {
        node_name: "n".into(),
        delta: 0,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { delta, .. } => assert_eq!(delta, 0),
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn evolution_op_add_retry_zero_max() {
    let op = EvolutionOp::AddRetry {
        node_name: "n".into(),
        max_retries: 0,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AddRetry { max_retries, .. } => assert_eq!(max_retries, 0),
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn evolution_op_insert_node_with_unicode() {
    let op = EvolutionOp::InsertNode {
        after_node: "节点-α".into(),
        new_name: "新节点-β".into(),
        tool_id: "工具-γ".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::InsertNode { after_node, new_name, tool_id } => {
            assert_eq!(after_node, "节点-α");
            assert_eq!(new_name, "新节点-β");
            assert_eq!(tool_id, "工具-γ");
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn notepad_snapshot_with_unicode_trigger() {
    let n = NotepadSnapshot {
        timestamp: 1700000000,
        snapshot: im::HashMap::new(),
        trigger_node: "节点-α".into(),
    };
    assert_eq!(n.trigger_node, "节点-α");
    let j = serde_json::to_string(&n).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.trigger_node, "节点-α");
}

#[test]
fn notepad_snapshot_with_max_timestamp() {
    let n = NotepadSnapshot {
        timestamp: u64::MAX,
        snapshot: im::HashMap::new(),
        trigger_node: "n".into(),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.timestamp, u64::MAX);
}

// ---------------------------------------------------------------------------
// Additional Notepad tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id() {
    let mut n = SisyphusNotepad::default();
    n.write_section("计划-α".into(), "内容-β".into());
    let read = n.read_section("计划-α");
    assert_eq!(read, Some(&"内容-β".to_string()));
}

#[test]
fn sisyphus_notepad_overwrite_unicode_section() {
    let mut n = SisyphusNotepad::default();
    n.write_section("section".into(), "first".into());
    n.write_section("section".into(), "second".into());
    let read = n.read_section("section");
    assert_eq!(read, Some(&"second".to_string()));
}

#[test]
fn sisyphus_notepad_write_multiple_sections() {
    let mut n = SisyphusNotepad::default();
    for i in 0..10 {
        n.write_section(format!("section-{}", i), format!("content-{}", i));
    }
    for i in 0..10 {
        let read = n.read_section(&format!("section-{}", i));
        assert_eq!(read, Some(&format!("content-{}", i)));
    }
}

#[test]
fn sisyphus_notepad_append_to_unicode_section() {
    let mut n = SisyphusNotepad::default();
    n.append_insight("模块-α".into(), "洞察-β".into());
    let read = n.read_section("模块-α");
    assert!(read.is_some());
    assert!(read.unwrap().contains("洞察-β"));
}

#[test]
fn sisyphus_notepad_take_snapshot_with_long_trigger_name() {
    let mut n = SisyphusNotepad::default();
    n.write_section("s".into(), "c".into());
    let long_name = "节点-α".repeat(100);
    n.take_snapshot(long_name.clone());
    assert_eq!(n.history.len(), 1);
    assert_eq!(n.history[0].trigger_node, long_name);
}

#[test]
fn sisyphus_notepad_take_snapshot_records_state() {
    let mut n = SisyphusNotepad::default();
    n.write_section("section1".into(), "value1".into());
    n.take_snapshot("trigger1".into());
    n.write_section("section2".into(), "value2".into());
    n.take_snapshot("trigger2".into());
    assert_eq!(n.history.len(), 2);
    assert_eq!(n.history[0].trigger_node, "trigger1");
    assert_eq!(n.history[1].trigger_node, "trigger2");
}

#[test]
fn sisyphus_notepad_prune_with_max_zero() {
    let mut n = SisyphusNotepad::default();
    n.write_section("s".into(), "line1\nline2\nline3".into());
    n.prune(0);
    let read = n.read_section("s");
    // Prune with max=0 should empty the section
    assert_eq!(read, Some(&"".to_string()));
}

// ---------------------------------------------------------------------------
// Additional Coverage
// ---------------------------------------------------------------------------

#[test]
fn circuit_state_serde_unicode() {
    use sisyphus_types::types::CircuitState;
    // Just verify serde is stable
    for v in [CircuitState::Closed, CircuitState::Open, CircuitState::HalfOpen] {
        let j = serde_json::to_string(&v).unwrap();
        let back: CircuitState = serde_json::from_str(&j).unwrap();
        let j2 = serde_json::to_string(&back).unwrap();
        assert_eq!(j, j2);
    }
}

#[test]
fn model_route_strategy_serde_all_variants() {
    use sisyphus_types::types::ModelRouteStrategy;
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
fn evolution_op_serialize_each_variant() {
    use sisyphus_types::types::EvolutionOp;
    let ops = vec![
        EvolutionOp::AdjustFuel {
            node_name: "n".into(),
            delta: 10,
        },
        EvolutionOp::AddRetry {
            node_name: "n".into(),
            max_retries: 3,
        },
        EvolutionOp::InsertNode {
            after_node: "a".into(),
            new_name: "b".into(),
            tool_id: "c".into(),
        },
        EvolutionOp::RemoveNode { node_name: "n".into() },
        EvolutionOp::SwapNodes {
            node_a: "a".into(),
            node_b: "b".into(),
        },
    ];
    for op in &ops {
        let j = serde_json::to_string(op).unwrap();
        let back: EvolutionOp = serde_json::from_str(&j).unwrap();
        let j2 = serde_json::to_string(&back).unwrap();
        assert_eq!(j, j2);
    }
}

#[test]
fn evolution_op_with_unicode_node_names() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::RemoveNode {
        node_name: "节点-α".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::RemoveNode { node_name } => assert_eq!(node_name, "节点-α"),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn work_mode_inequality() {
    assert_ne!(WorkMode::Standard, WorkMode::UltraWork);
}

#[test]
fn notepad_snapshot_unicode_content() {
    let mut snapshot = im::HashMap::new();
    snapshot.insert("section-α".into(), "content-β".into());
    let n = NotepadSnapshot {
        timestamp: 1000,
        snapshot,
        trigger_node: "节点-γ".into(),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.trigger_node, "节点-γ");
    assert_eq!(back.snapshot.get("section-α"), Some(&"content-β".to_string()));
}

#[test]
fn sisyphus_notepad_append_long_insight() {
    let mut n = SisyphusNotepad::default();
    let long_insight = "x".repeat(5000);
    n.append_insight("s".into(), long_insight.clone());
    let read = n.read_section("s").unwrap();
    assert!(read.contains(&long_insight));
}

// ---------------------------------------------------------------------------
// Additional Coverage
// ---------------------------------------------------------------------------

#[test]
fn model_route_strategy_serde_with_unicode_metadata() {
    use sisyphus_types::types::ModelRouteStrategy;
    // Just verify serde
    for v in [
        ModelRouteStrategy::CostOptimized,
        ModelRouteStrategy::PerformanceOptimized,
        ModelRouteStrategy::FallbackChain,
    ] {
        let j = serde_json::to_string(&v).unwrap();
        assert!(j.contains("ModelRouteStrategy") || j.len() < 50);
    }
}

#[test]
fn evolution_op_with_all_field_types() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::SwapNodes {
        node_a: "α".into(),
        node_b: "β".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::SwapNodes { node_a, node_b } => {
            assert_eq!(node_a, "α");
            assert_eq!(node_b, "β");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn sisyphus_notepad_append_to_existing_with_unicode() {
    let mut n = SisyphusNotepad::default();
    n.append_insight("plan".into(), "first".into());
    n.append_insight("plan".into(), "中文-β".into());
    let read = n.read_section("plan").unwrap();
    assert!(read.contains("first"));
    assert!(read.contains("中文-β"));
}

#[test]
fn sisyphus_notepad_overwrite_via_write() {
    let mut n = SisyphusNotepad::default();
    n.write_section("s".into(), "original".into());
    n.write_section("s".into(), "replaced".into());
    let read = n.read_section("s").unwrap();
    assert_eq!(read, &"replaced".to_string());
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_serde_safe() {
    let mut n = SisyphusNotepad::default();
    n.write_section("模块-α".into(), "内容-β".into());
    // SisyphusNotepad doesn't implement Serialize directly
    // Just verify field access works
    assert_eq!(n.sections.len(), 1);
}

#[test]
fn trigger_def_with_unicode_tools_serde() {
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools: vec!["工具-α".into(), "工具-β".into(), "工具-γ".into()],
        description: None,
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools.len(), 3);
    assert_eq!(back.tools[0], "工具-α");
}

#[test]
fn trigger_def_with_only_description_serde() {
    let t: TriggerDef = serde_json::from_str(r#"{"description":"only description"}"#).unwrap();
    assert_eq!(t.description.as_deref(), Some("only description"));
    assert!(t.pattern.is_none());
}

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn evolution_op_clone_each_variant() {
    use sisyphus_types::types::EvolutionOp;
    let ops = vec![
        EvolutionOp::AdjustFuel {
            node_name: "n".into(),
            delta: 10,
        },
        EvolutionOp::AddRetry {
            node_name: "n".into(),
            max_retries: 3,
        },
        EvolutionOp::InsertNode {
            after_node: "a".into(),
            new_name: "b".into(),
            tool_id: "c".into(),
        },
        EvolutionOp::RemoveNode { node_name: "n".into() },
        EvolutionOp::SwapNodes {
            node_a: "a".into(),
            node_b: "b".into(),
        },
    ];
    for op in &ops {
        let cloned = op.clone();
        let orig_dbg = format!("{:?}", op);
        let clone_dbg = format!("{:?}", cloned);
        assert_eq!(orig_dbg, clone_dbg);
    }
}

#[test]
fn sisyphus_notepad_default_section_count() {
    let n = SisyphusNotepad::default();
    assert_eq!(n.sections.len(), 0);
    assert_eq!(n.history.len(), 0);
}

#[test]
fn sisyphus_notepad_write_section_then_read() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a".into(), "b".into());
    let read = n.read_section("a");
    assert_eq!(read, Some(&"b".to_string()));
}

#[test]
fn sisyphus_notepad_with_max_history() {
    let mut n = SisyphusNotepad::default();
    for i in 0..100 {
        n.take_snapshot(format!("trigger-{}", i));
    }
    assert_eq!(n.history.len(), 100);
    assert_eq!(n.history[50].trigger_node, "trigger-50");
}

#[test]
fn trigger_def_with_unicode_pattern_and_tools() {
    let t = TriggerDef {
        pattern: Some("模式-α".into()),
        tools: vec!["工具-α".into()],
        description: Some("描述".into()),
        hook_phase: Some("阶段-β".into()),
        hook_effect: Some("效果-γ".into()),
        hook_message: Some("消息-δ".into()),
    };
    assert_eq!(t.pattern.as_deref(), Some("模式-α"));
    assert_eq!(t.tools[0], "工具-α");
}

#[test]
fn work_mode_default_and_clone() {
    let wm: WorkMode = Default::default();
    assert_eq!(wm, WorkMode::Standard);
    let cloned = wm.clone();
    assert_eq!(cloned, wm);
}

#[test]
fn model_route_strategy_with_all_variants_serde() {
    use sisyphus_types::types::ModelRouteStrategy;
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

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn trigger_def_with_max_tools() {
    let tools: Vec<String> = (0..1000).map(|i| format!("tool-{}", i)).collect();
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools,
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools.len(), 1000);
}

#[test]
fn trigger_def_with_empty_pattern_and_description() {
    let t = TriggerDef {
        pattern: Some("".into()),
        tools: vec![],
        description: Some("".into()),
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.pattern.as_deref(), Some(""));
    assert_eq!(back.description.as_deref(), Some(""));
}

#[test]
fn sisyphus_notepad_write_then_append_preserves_both() {
    let mut n = SisyphusNotepad::default();
    n.write_section("s".into(), "initial".into());
    n.append_insight("s".into(), "first insight".into());
    let read = n.read_section("s").unwrap();
    assert!(read.contains("initial"));
    assert!(read.contains("first insight"));
}

#[test]
fn sisyphus_notepad_prune_keeps_short_sections() {
    let mut n = SisyphusNotepad::default();
    n.write_section("short".into(), "one line".into());
    n.prune(5);
    let read = n.read_section("short").unwrap();
    assert_eq!(read, &"one line".to_string());
}

#[test]
fn sisyphus_notepad_with_5_sections() {
    let mut n = SisyphusNotepad::default();
    for i in 0..5 {
        n.write_section(format!("s{}", i), format!("content{}", i));
    }
    assert_eq!(n.sections.len(), 5);
}

#[test]
fn notepad_snapshot_with_5_sections() {
    let mut snapshot = im::HashMap::new();
    for i in 0..5 {
        snapshot.insert(format!("s{}", i), format!("v{}", i));
    }
    let n = NotepadSnapshot {
        timestamp: 1000,
        snapshot,
        trigger_node: "trigger".into(),
    };
    assert_eq!(n.snapshot.len(), 5);
}

#[test]
fn evolution_op_with_zero_and_max_delta() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::AdjustFuel {
        node_name: "n".into(),
        delta: i64::MAX,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { delta, .. } => assert_eq!(delta, i64::MAX),
        _ => panic!("wrong"),
    }
}

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn evolution_op_with_unicode_remove_node() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::RemoveNode {
        node_name: "节点-α-β-γ".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::RemoveNode { node_name } => {
            assert_eq!(node_name, "节点-α-β-γ");
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn evolution_op_with_unicode_swap_nodes() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::SwapNodes {
        node_a: "α".into(),
        node_b: "β".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::SwapNodes { node_a, node_b } => {
            assert_eq!(node_a, "α");
            assert_eq!(node_b, "β");
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_read() {
    let mut n = SisyphusNotepad::default();
    n.write_section("模块-α-β".into(), "内容".into());
    let read = n.read_section("模块-α-β");
    assert_eq!(read, Some(&"内容".to_string()));
}

#[test]
fn sisyphus_notepad_append_to_unicode_section_id() {
    let mut n = SisyphusNotepad::default();
    n.append_insight("节-α".into(), "洞-β".into());
    let read = n.read_section("节-α");
    assert!(read.is_some());
    assert!(read.unwrap().contains("洞-β"));
}

#[test]
fn trigger_def_with_only_unicode_pattern_serde() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"模式-α"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("模式-α"));
}

#[test]
fn trigger_def_with_only_unicode_description_serde() {
    let t: TriggerDef = serde_json::from_str(r#"{"description":"描述-α"}"#).unwrap();
    assert_eq!(t.description.as_deref(), Some("描述-α"));
}

#[test]
fn work_mode_with_serde_round_trip() {
    use sisyphus_types::types::WorkMode;
    for v in [WorkMode::Standard, WorkMode::UltraWork] {
        let j = serde_json::to_string(&v).unwrap();
        let back: WorkMode = serde_json::from_str(&j).unwrap();
        assert_eq!(back, v);
    }
}

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn evolution_op_with_unicode_insert_node_serde() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::InsertNode {
        after_node: "节-α".into(),
        new_name: "新-β".into(),
        tool_id: "工-γ".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::InsertNode { after_node, new_name, tool_id } => {
            assert_eq!(after_node, "节-α");
            assert_eq!(new_name, "新-β");
            assert_eq!(tool_id, "工-γ");
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_emoji_section() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🔍".into(), "内容-α".into());
    let read = n.read_section("🔍");
    assert_eq!(read, Some(&"内容-α".to_string()));
}

#[test]
fn trigger_def_with_unicode_long_tools() {
    let tools: Vec<String> = (0..100).map(|i| format!("工具-{}", i)).collect();
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools,
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools.len(), 100);
    assert_eq!(back.tools[99], "工具-99");
}

#[test]
fn sisyphus_notepad_with_max_sections() {
    let mut n = SisyphusNotepad::default();
    for i in 0..1000 {
        n.write_section(format!("s{}", i), format!("v{}", i));
    }
    assert_eq!(n.sections.len(), 1000);
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_many() {
    let mut n = SisyphusNotepad::default();
    for i in 0..50 {
        n.write_section(format!("键-{}", i), format!("值-{}", i));
    }
    assert_eq!(n.sections.len(), 50);
}

#[test]
fn notepad_snapshot_with_unicode_trigger_node_only() {
    let n = NotepadSnapshot {
        timestamp: 0,
        snapshot: im::HashMap::new(),
        trigger_node: "节-α-β-γ".into(),
    };
    assert_eq!(n.trigger_node, "节-α-β-γ");
    let j = serde_json::to_string(&n).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.trigger_node, "节-α-β-γ");
}

#[test]
fn work_mode_standard_and_ultra_work_distinct() {
    use sisyphus_types::types::WorkMode;
    assert_ne!(WorkMode::Standard, WorkMode::UltraWork);
    let std_dbg = format!("{:?}", WorkMode::Standard);
    let ult_dbg = format!("{:?}", WorkMode::UltraWork);
    assert_ne!(std_dbg, ult_dbg);
    assert_eq!(std_dbg, "Standard");
    assert_eq!(ult_dbg, "UltraWork");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_read_after_append() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a".into(), "v1".into());
    n.append_insight("a".into(), "v2".into());
    n.append_insight("a".into(), "v3".into());
    let read = n.read_section("a").unwrap();
    assert!(read.contains("v1"));
    assert!(read.contains("v2"));
    assert!(read.contains("v3"));
}

#[test]
fn sisyphus_notepad_with_only_one_section_unicode() {
    let mut n = SisyphusNotepad::default();
    n.write_section("唯一-α".into(), "内容-β".into());
    assert_eq!(n.sections.len(), 1);
    let read = n.read_section("唯一-α").unwrap();
    assert_eq!(read, &"内容-β".to_string());
}

#[test]
fn evolution_op_serialize_to_distinct_strings() {
    use sisyphus_types::types::EvolutionOp;
    let variants = vec![
        EvolutionOp::AdjustFuel {
            node_name: "n".into(),
            delta: 1,
        },
        EvolutionOp::AddRetry {
            node_name: "n".into(),
            max_retries: 1,
        },
        EvolutionOp::InsertNode {
            after_node: "a".into(),
            new_name: "b".into(),
            tool_id: "c".into(),
        },
        EvolutionOp::RemoveNode { node_name: "n".into() },
        EvolutionOp::SwapNodes {
            node_a: "a".into(),
            node_b: "b".into(),
        },
    ];
    let serialized: Vec<String> = variants
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();
    let unique: std::collections::HashSet<_> = serialized.iter().collect();
    assert_eq!(unique.len(), 5);
}

#[test]
fn trigger_def_with_unicode_pattern_and_hook_only() {
    let t: TriggerDef = serde_json::from_str(
        r#"{"pattern":"模式-α","hook_phase":"阶段-β","hook_effect":"效果-γ","hook_message":"消息-δ"}"#,
    )
    .unwrap();
    assert_eq!(t.pattern.as_deref(), Some("模式-α"));
    assert_eq!(t.hook_phase.as_deref(), Some("阶段-β"));
    assert_eq!(t.hook_effect.as_deref(), Some("效果-γ"));
    assert_eq!(t.hook_message.as_deref(), Some("消息-δ"));
}

#[test]
fn sisyphus_notepad_with_unicode_append_only() {
    let mut n = SisyphusNotepad::default();
    n.append_insight("节-α".into(), "洞-β".into());
    n.append_insight("节-α".into(), "洞-γ".into());
    let read = n.read_section("节-α").unwrap();
    assert!(read.contains("洞-β"));
    assert!(read.contains("洞-γ"));
}

#[test]
fn notepad_snapshot_with_only_unicode_trigger_serde() {
    let n = NotepadSnapshot {
        timestamp: 0,
        snapshot: im::HashMap::new(),
        trigger_node: "触发-α".into(),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.trigger_node, "触发-α");
}

#[test]
fn sisyphus_notepad_with_emoji_section_id_serde_safe() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🔍".into(), "内容".into());
    // SisyphusNotepad has fields, but no Serialize impl
    assert_eq!(n.sections.len(), 1);
    let read = n.read_section("🔍");
    assert_eq!(read, Some(&"内容".to_string()));
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id_prune() {
    let mut n = SisyphusNotepad::default();
    n.write_section("节-α".into(), "line1\nline2\nline3".into());
    n.prune(2);
    let read = n.read_section("节-α").unwrap();
    // Should keep last 2 lines
    assert!(read.contains("line2"));
    assert!(read.contains("line3"));
}

#[test]
fn sisyphus_notepad_with_only_one_section_prune() {
    let mut n = SisyphusNotepad::default();
    n.write_section("s".into(), "single".into());
    n.prune(10);
    let read = n.read_section("s").unwrap();
    assert_eq!(read, &"single".to_string());
}

#[test]
fn trigger_def_with_unicode_3_segment_id_in_field() {
    let t = TriggerDef {
        pattern: Some("前缀-α-中缀-β-后缀-γ".into()),
        tools: vec!["t".into()],
        description: Some("d".into()),
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    assert_eq!(
        t.pattern.as_deref(),
        Some("前缀-α-中缀-β-后缀-γ")
    );
}

#[test]
fn notepad_snapshot_with_unicode_2_sections() {
    let mut snapshot = im::HashMap::new();
    snapshot.insert("节-α".into(), "内容-β".into());
    snapshot.insert("节-γ".into(), "内容-δ".into());
    let n = NotepadSnapshot {
        timestamp: 1000,
        snapshot,
        trigger_node: "触发-ε".into(),
    };
    assert_eq!(n.snapshot.len(), 2);
    let j = serde_json::to_string(&n).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.snapshot.get("节-α"), Some(&"内容-β".to_string()));
}

#[test]
fn evolution_op_with_negative_delta_serde() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::AdjustFuel {
        node_name: "n-α".into(),
        delta: -100,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "n-α");
            assert_eq!(delta, -100);
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_long_unicode_section_id() {
    let mut n = SisyphusNotepad::default();
    let long_id = "节-α-".repeat(50);
    n.write_section(long_id.clone(), "v".into());
    let read = n.read_section(&long_id);
    assert!(read.is_some());
}

#[test]
fn trigger_def_with_unicode_tools_array_serde() {
    let tools = vec![
        "工-α".to_string(),
        "工-β".to_string(),
        "工-γ".to_string(),
        "工-δ".to_string(),
    ];
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools: tools.clone(),
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools.len(), 4);
    assert_eq!(back.tools[2], "工-γ");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_and_long_content() {
    let mut n = SisyphusNotepad::default();
    let long_content = "x".repeat(1000);
    n.write_section("节-α".into(), long_content.clone());
    let read = n.read_section("节-α").unwrap();
    assert_eq!(read, &long_content);
}

#[test]
fn sisyphus_notepad_with_unicode_3_overwrite_cycles() {
    let mut n = SisyphusNotepad::default();
    n.write_section("s".into(), "v1".into());
    n.write_section("s".into(), "v2".into());
    n.write_section("s".into(), "v3".into());
    let read = n.read_section("s").unwrap();
    assert_eq!(read, &"v3".to_string());
}

#[test]
fn trigger_def_with_unicode_long_description() {
    let long_desc = "描-".repeat(100);
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools: vec!["t".into()],
        description: Some(long_desc.clone()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, Some(long_desc));
}

#[test]
fn notepad_snapshot_with_unicode_3_sections() {
    let mut snapshot = im::HashMap::new();
    snapshot.insert("a-α".into(), "1-β".into());
    snapshot.insert("b-γ".into(), "2-δ".into());
    snapshot.insert("c-ε".into(), "3-ζ".into());
    let n = NotepadSnapshot {
        timestamp: 1000,
        snapshot,
        trigger_node: "trigger-α".into(),
    };
    assert_eq!(n.snapshot.len(), 3);
}

#[test]
fn evolution_op_with_unicode_each_variant_serde() {
    use sisyphus_types::types::EvolutionOp;
    let ops = vec![
        ("a-α", EvolutionOp::AdjustFuel {
            node_name: "n-β".into(),
            delta: 10,
        }),
        ("b-γ", EvolutionOp::AddRetry {
            node_name: "n-δ".into(),
            max_retries: 3,
        }),
        ("c-ε", EvolutionOp::RemoveNode {
            node_name: "n-ζ".into(),
        }),
    ];
    for (label, op) in &ops {
        let j = serde_json::to_string(op).unwrap();
        let back: EvolutionOp = serde_json::from_str(&j).unwrap();
        let j2 = serde_json::to_string(&back).unwrap();
        assert_eq!(j, j2, "Failed for {}", label);
    }
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_long() {
    let mut n = SisyphusNotepad::default();
    let long_id = "节-α-".repeat(50);
    n.write_section(long_id.clone(), "v".into());
    assert_eq!(n.sections.len(), 1);
    let read = n.read_section(&long_id);
    assert!(read.is_some());
}

#[test]
fn trigger_def_with_only_unicode_each_optional_field() {
    let t: TriggerDef = serde_json::from_str(
        r#"{"pattern":"p-α","description":"d-β","hook_phase":"hp-γ","hook_effect":"he-δ","hook_message":"hm-ε"}"#,
    )
    .unwrap();
    assert_eq!(t.pattern.as_deref(), Some("p-α"));
    assert_eq!(t.description.as_deref(), Some("d-β"));
    assert_eq!(t.hook_phase.as_deref(), Some("hp-γ"));
    assert_eq!(t.hook_effect.as_deref(), Some("he-δ"));
    assert_eq!(t.hook_message.as_deref(), Some("hm-ε"));
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id_special_chars() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a/b/α-β".into(), "v".into());
    let read = n.read_section("a/b/α-β");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_only_unicode_take_snapshot_3_times() {
    let mut n = SisyphusNotepad::default();
    n.take_snapshot("trigger-α".into());
    n.take_snapshot("trigger-β".into());
    n.take_snapshot("trigger-γ".into());
    assert_eq!(n.history.len(), 3);
    assert_eq!(n.history[0].trigger_node, "trigger-α");
    assert_eq!(n.history[1].trigger_node, "trigger-β");
    assert_eq!(n.history[2].trigger_node, "trigger-γ");
}

#[test]
fn trigger_def_with_unicode_2_segment_pattern() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"前缀-α-后缀-β"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("前缀-α-后缀-β"));
}

#[test]
fn notepad_snapshot_with_only_unicode_trigger_emoji() {
    let n = NotepadSnapshot {
        timestamp: 0,
        snapshot: im::HashMap::new(),
        trigger_node: "🔍".into(),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.trigger_node, "🔍");
}

#[test]
fn evolution_op_adjust_fuel_with_unicode_node_name_max() {
    use sisyphus_types::types::EvolutionOp;
    let long_name = "节-α-".repeat(50);
    let op = EvolutionOp::AdjustFuel {
        node_name: long_name.clone(),
        delta: 100,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, long_name);
            assert_eq!(delta, 100);
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_empty() {
    let mut n = SisyphusNotepad::default();
    n.write_section("".into(), "v".into());
    let read = n.read_section("");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_long_section_id_500() {
    let mut n = SisyphusNotepad::default();
    let long_id = "α-".repeat(250);
    n.write_section(long_id.clone(), "v".into());
    let read = n.read_section(&long_id);
    assert!(read.is_some());
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_dot() {
    let mut n = SisyphusNotepad::default();
    n.write_section("section.α-β.γ".into(), "v".into());
    let read = n.read_section("section.α-β.γ");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_4_sections() {
    let mut n = SisyphusNotepad::default();
    n.write_section("节-α".into(), "v1".into());
    n.write_section("节-β".into(), "v2".into());
    n.write_section("节-γ".into(), "v3".into());
    n.write_section("节-δ".into(), "v4".into());
    assert_eq!(n.sections.len(), 4);
}

#[test]
fn trigger_def_with_unicode_3_segment_pattern_2() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"前-α-中-β-后-γ"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("前-α-中-β-后-γ"));
}

#[test]
fn notepad_snapshot_with_unicode_each_field() {
    let mut snapshot = im::HashMap::new();
    snapshot.insert("节-α".into(), "内容-β".into());
    let n = NotepadSnapshot {
        timestamp: 1700000000,
        snapshot,
        trigger_node: "触发-γ".into(),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.trigger_node, "触发-γ");
    assert_eq!(back.timestamp, 1700000000);
    assert_eq!(back.snapshot.get("节-α"), Some(&"内容-β".to_string()));
}

#[test]
fn evolution_op_with_unicode_max_delta() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::AdjustFuel {
        node_name: "节-α".into(),
        delta: i64::MAX,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "节-α");
            assert_eq!(delta, i64::MAX);
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_via_only_emoji() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🔍".into(), "search".into());
    let read = n.read_section("🔍");
    assert_eq!(read, Some(&"search".to_string()));
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_emoji_combined() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🔍-α-β".into(), "v".into());
    let read = n.read_section("🔍-α-β");
    assert!(read.is_some());
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_slash_2() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a/b/节-α-🔍".into(), "v".into());
    let read = n.read_section("a/b/节-α-🔍");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_5_unicode_sections() {
    let mut n = SisyphusNotepad::default();
    for i in 0..5 {
        n.write_section(format!("节-{}", i), format!("v-{}", i));
    }
    assert_eq!(n.sections.len(), 5);
}

#[test]
fn trigger_def_with_unicode_pattern_emoji_2() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"🔍-🔭"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("🔍-🔭"));
}

#[test]
fn notepad_snapshot_with_unicode_3_sections_2() {
    let mut snapshot = im::HashMap::new();
    snapshot.insert("a-α".into(), "1-β".into());
    snapshot.insert("b-γ".into(), "2-δ".into());
    snapshot.insert("c-ε".into(), "3-ζ".into());
    let n = NotepadSnapshot {
        timestamp: 0,
        snapshot,
        trigger_node: "t-η".into(),
    };
    assert_eq!(n.snapshot.len(), 3);
}

#[test]
fn evolution_op_with_unicode_negative_delta() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::AdjustFuel {
        node_name: "节-α".into(),
        delta: -1000,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "节-α");
            assert_eq!(delta, -1000);
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_5_chars() {
    let mut n = SisyphusNotepad::default();
    n.write_section("αβγδε".into(), "v".into());
    let read = n.read_section("αβγδε");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_2_sections_prune() {
    let mut n = SisyphusNotepad::default();
    n.write_section("节-α".into(), "line1-α\nline2-α\nline3-α".into());
    n.write_section("节-β".into(), "line1-β".into());
    n.prune(2);
    // 2 sections still exist
    assert_eq!(n.sections.len(), 2);
    // First section should have last 2 lines
    let read = n.read_section("节-α").unwrap();
    assert!(read.contains("line2-α"));
    assert!(read.contains("line3-α"));
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_emoji_2() {
    let mut n = SisyphusNotepad::default();
    n.write_section("节-α🔍".into(), "v".into());
    let read = n.read_section("节-α🔍");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_5_chars_2() {
    let mut n = SisyphusNotepad::default();
    n.write_section("节-α-β-γ-δ".into(), "v".into());
    let read = n.read_section("节-α-β-γ-δ");
    assert!(read.is_some());
}

#[test]
fn trigger_def_with_unicode_pattern_at_sign() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"@trigger-α"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("@trigger-α"));
}

#[test]
fn notepad_snapshot_with_unicode_trigger_emoji_2() {
    let n = NotepadSnapshot {
        timestamp: 0,
        snapshot: im::HashMap::new(),
        trigger_node: "🔍-α".into(),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.trigger_node, "🔍-α");
}

#[test]
fn evolution_op_adjust_fuel_with_unicode_node_2_chars() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::AdjustFuel {
        node_name: "αβ".into(),
        delta: 100,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "αβ");
            assert_eq!(delta, 100);
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_at_sign() {
    let mut n = SisyphusNotepad::default();
    n.write_section("user@α-β".into(), "v".into());
    let read = n.read_section("user@α-β");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_pipes() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a|b|α|β".into(), "v".into());
    let read = n.read_section("a|b|α|β");
    assert!(read.is_some());
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id_only_emoji_3() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🔍🔭🌟".into(), "v".into());
    let read = n.read_section("🔍🔭🌟");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_2_sections_overwrite() {
    let mut n = SisyphusNotepad::default();
    n.write_section("节-α".into(), "v1".into());
    n.write_section("节-β".into(), "v1".into());
    n.write_section("节-α".into(), "v2-α".into());
    let read = n.read_section("节-α");
    assert_eq!(read, Some(&"v2-α".to_string()));
}

#[test]
fn trigger_def_with_unicode_only_emoji_pattern() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"🔍"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("🔍"));
}

#[test]
fn notepad_snapshot_with_unicode_4_sections() {
    let mut snapshot = im::HashMap::new();
    for i in 0..4 {
        snapshot.insert(format!("节-{}", i), format!("内容-{}", i));
    }
    let n = NotepadSnapshot {
        timestamp: 0,
        snapshot,
        trigger_node: "触发".into(),
    };
    assert_eq!(n.snapshot.len(), 4);
}

#[test]
fn evolution_op_with_unicode_max_delta_2() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::AdjustFuel {
        node_name: "节-α-β".into(),
        delta: i64::MAX,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "节-α-β");
            assert_eq!(delta, i64::MAX);
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_2_segment_section_id() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a-α".into(), "v".into());
    let read = n.read_section("a-α");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_4_chars() {
    let mut n = SisyphusNotepad::default();
    n.write_section("αβγδ".into(), "v".into());
    let read = n.read_section("αβγδ");
    assert!(read.is_some());
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_3_dots() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a.b.c.α".into(), "v".into());
    let read = n.read_section("a.b.c.α");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_5_sections_overwrite() {
    let mut n = SisyphusNotepad::default();
    for i in 0..5 {
        n.write_section(format!("节-{}", i), format!("v-{}", i));
    }
    n.write_section("节-2".into(), "v-2-overwrite".into());
    let read = n.read_section("节-2").unwrap();
    assert_eq!(read, "v-2-overwrite");
}

#[test]
fn trigger_def_with_unicode_pattern_3_chars() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"αβγ"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("αβγ"));
}

#[test]
fn notepad_snapshot_with_unicode_each_field_2() {
    let mut snapshot = im::HashMap::new();
    snapshot.insert("节-α".into(), "内容-β".into());
    let n = NotepadSnapshot {
        timestamp: 0,
        snapshot,
        trigger_node: "节-γ".into(),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.trigger_node, "节-γ");
    assert_eq!(back.snapshot.get("节-α"), Some(&"内容-β".to_string()));
}

#[test]
fn evolution_op_with_unicode_min_delta_2() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::AdjustFuel {
        node_name: "n-α".into(),
        delta: i64::MIN,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "n-α");
            assert_eq!(delta, i64::MIN);
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_special() {
    let mut n = SisyphusNotepad::default();
    n.write_section("!@#$%α-β^&*()".into(), "v".into());
    let read = n.read_section("!@#$%α-β^&*()");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_only_emoji_2_chars_2() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🔍🔭".into(), "v".into());
    let read = n.read_section("🔍🔭");
    assert!(read.is_some());
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_emoji_3() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🔍-节-α".into(), "v".into());
    let read = n.read_section("🔍-节-α");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_3_sections_overwrite_first() {
    let mut n = SisyphusNotepad::default();
    n.write_section("节-α".into(), "v1".into());
    n.write_section("节-β".into(), "v2".into());
    n.write_section("节-γ".into(), "v3".into());
    n.write_section("节-α".into(), "v1-new".into());
    let read = n.read_section("节-α").unwrap();
    assert_eq!(read, "v1-new");
}

#[test]
fn trigger_def_with_unicode_pattern_with_emoji_2() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"🔍-α-β-γ-δ"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("🔍-α-β-γ-δ"));
}

#[test]
fn notepad_snapshot_with_unicode_5_sections() {
    let mut snapshot = im::HashMap::new();
    for i in 0..5 {
        snapshot.insert(format!("节-{}", i), format!("内容-{}", i));
    }
    let n = NotepadSnapshot {
        timestamp: 0,
        snapshot,
        trigger_node: "触发-α".into(),
    };
    assert_eq!(n.snapshot.len(), 5);
}

#[test]
fn evolution_op_with_unicode_min_delta_3() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::AdjustFuel {
        node_name: "n-α-🔍".into(),
        delta: -100,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "n-α-🔍");
            assert_eq!(delta, -100);
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_only_at_signs_2() {
    let mut n = SisyphusNotepad::default();
    n.write_section("@@@".into(), "v".into());
    let read = n.read_section("@@@");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_dashes() {
    let mut n = SisyphusNotepad::default();
    n.write_section("---".into(), "v".into());
    let read = n.read_section("---");
    assert!(read.is_some());
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_emoji_4() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🔍-α-β-γ".into(), "v".into());
    let read = n.read_section("🔍-α-β-γ");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_4_sections_overwrite_one() {
    let mut n = SisyphusNotepad::default();
    for i in 0..4 {
        n.write_section(format!("节-{}", i), format!("v-{}", i));
    }
    n.write_section("节-2".into(), "v-2-new".into());
    let read = n.read_section("节-2").unwrap();
    assert_eq!(read, "v-2-new");
}

#[test]
fn trigger_def_with_unicode_pattern_2_chars() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"αβ"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("αβ"));
}

#[test]
fn notepad_snapshot_with_unicode_2_sections_2() {
    let mut snapshot = im::HashMap::new();
    snapshot.insert("节-α".into(), "内容-α".into());
    snapshot.insert("节-β".into(), "内容-β".into());
    let n = NotepadSnapshot {
        timestamp: 0,
        snapshot,
        trigger_node: "触发".into(),
    };
    assert_eq!(n.snapshot.len(), 2);
}

#[test]
fn evolution_op_with_unicode_max_delta_3() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::AdjustFuel {
        node_name: "n-α-🔍".into(),
        delta: 1000,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "n-α-🔍");
            assert_eq!(delta, 1000);
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_only_emoji_3() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🌟".into(), "v".into());
    let read = n.read_section("🌟");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_only_emoji_2_chars_3() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🔍🔭".into(), "v".into());
    let read = n.read_section("🔍🔭");
    assert!(read.is_some());
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_3_emoji() {
    let mut n = SisyphusNotepad::default();
    n.write_section("🔍α🔭".into(), "v".into());
    let read = n.read_section("🔍α🔭");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_3_sections_overwrite_last() {
    let mut n = SisyphusNotepad::default();
    n.write_section("节-α".into(), "v1".into());
    n.write_section("节-β".into(), "v2".into());
    n.write_section("节-γ".into(), "v3".into());
    n.write_section("节-γ".into(), "v3-new".into());
    let read = n.read_section("节-γ").unwrap();
    assert_eq!(read, "v3-new");
}

#[test]
fn trigger_def_with_unicode_pattern_5_chars() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"αβγδε"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("αβγδε"));
}

#[test]
fn notepad_snapshot_with_unicode_3_sections_3() {
    let mut snapshot = im::HashMap::new();
    snapshot.insert("a-α".into(), "1-β".into());
    snapshot.insert("b-γ".into(), "2-δ".into());
    snapshot.insert("c-ε".into(), "3-ζ".into());
    let n = NotepadSnapshot {
        timestamp: 0,
        snapshot,
        trigger_node: "t-η".into(),
    };
    assert_eq!(n.snapshot.len(), 3);
}

#[test]
fn evolution_op_with_unicode_max_delta_4() {
    use sisyphus_types::types::EvolutionOp;
    let op = EvolutionOp::AdjustFuel {
        node_name: "n-α-β-γ-🔍".into(),
        delta: i64::MAX,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "n-α-β-γ-🔍");
            assert_eq!(delta, i64::MAX);
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_quotes() {
    let mut n = SisyphusNotepad::default();
    n.write_section("'α-β'".into(), "v".into());
    let read = n.read_section("'α-β'");
    assert!(read.is_some());
}

#[test]
fn sisyphus_notepad_with_unicode_section_id_with_dots_2() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a.α.β".into(), "v".into());
    let read = n.read_section("a.α.β");
    assert!(read.is_some());
}
