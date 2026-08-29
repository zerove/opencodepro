//! Integration tests for sisyphus-types `types` module (top-level
//! flat data types). Covers [`Feedback`], [`PluginDescriptor`],
//! [`VectorRecord`], [`AgentMessage`], [`CircuitState`],
//! [`BenchmarkRun`], [`PlatformConfig`], [`WorkMode`],
//! [`CustomToolDefinition`], and [`TriggerDef`].

use sisyphus_types::{
    AgentMessage, BenchmarkRun, CircuitState, CustomToolDefinition, Feedback, PlatformConfig,
    PluginDescriptor, TriggerDef, VectorRecord, WorkMode,
};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Feedback
// ---------------------------------------------------------------------------

#[test]
fn feedback_field_assignment() {
    let f = Feedback {
        agent_id: "a1".into(),
        original_request: "do X".into(),
        original_output: "Y1".into(),
        corrected_output: "Y2".into(),
        timestamp: 12345,
        tags: vec!["k1".into()],
    };
    assert_eq!(f.agent_id, "a1");
    assert_eq!(f.corrected_output, "Y2");
}

#[test]
fn feedback_serde() {
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o1".into(),
        corrected_output: "o2".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "a");
}

// ---------------------------------------------------------------------------
// PluginDescriptor
// ---------------------------------------------------------------------------

#[test]
fn plugin_descriptor_field_assignment() {
    let p = PluginDescriptor {
        name: "plug-1".into(),
        version: "0.1.0".into(),
        wasm_path: PathBuf::from("/tmp/x.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    assert_eq!(p.name, "plug-1");
    assert_eq!(p.wasm_path, PathBuf::from("/tmp/x.wasm"));
}

#[test]
fn plugin_descriptor_serde() {
    let p = PluginDescriptor {
        name: "p".into(),
        version: "v".into(),
        wasm_path: PathBuf::from("/a/b.wasm"),
        tool_id: "t".into(),
        fuel_limit: 500,
        checksum: None,
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "p");
    assert_eq!(back.wasm_path, PathBuf::from("/a/b.wasm"));
}

// ---------------------------------------------------------------------------
// VectorRecord
// ---------------------------------------------------------------------------

#[test]
fn vector_record_field_assignment() {
    let v = VectorRecord {
        key: "k1".into(),
        embedding: vec![0.1, 0.2, 0.3],
        metadata: serde_json::json!({"src": "test"}),
        timestamp: 12345,
    };
    assert_eq!(v.key, "k1");
    assert_eq!(v.embedding.len(), 3);
    assert_eq!(v.timestamp, 12345);
}

#[test]
fn vector_record_serde() {
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![1.0, 2.0],
        metadata: serde_json::json!(null),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.key, "k");
    assert_eq!(back.embedding.len(), 2);
}

// ---------------------------------------------------------------------------
// AgentMessage
// ---------------------------------------------------------------------------

#[test]
fn agent_message_field_assignment() {
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({}),
        correlation_id: "c-1".into(),
    };
    assert_eq!(m.from, "a");
    assert_eq!(m.kind, "request");
}

#[test]
fn agent_message_serde() {
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "publish".into(),
        payload: serde_json::json!(null),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.kind, "publish");
}

// ---------------------------------------------------------------------------
// CircuitState
// ---------------------------------------------------------------------------

#[test]
fn circuit_state_variants_distinct() {
    let mut seen = std::collections::HashSet::new();
    for s in [CircuitState::Closed, CircuitState::Open, CircuitState::HalfOpen] {
        let d = format!("{s:?}");
        assert!(seen.insert(d.clone()));
    }
}

#[test]
fn circuit_state_serde() {
    for s in [CircuitState::Closed, CircuitState::Open, CircuitState::HalfOpen] {
        let j = serde_json::to_string(&s).unwrap();
        let back: CircuitState = serde_json::from_str(&j).unwrap();
        assert_eq!(format!("{back:?}"), format!("{s:?}"));
    }
}

// ---------------------------------------------------------------------------
// BenchmarkRun
// ---------------------------------------------------------------------------

#[test]
fn benchmark_run_field_assignment() {
    let b = BenchmarkRun {
        name: "bench1".into(),
        timestamp: 12345,
        duration_secs: 0.5,
        success: true,
        metrics: Default::default(),
    };
    assert_eq!(b.name, "bench1");
    assert!(b.success);
}

#[test]
fn benchmark_run_serde() {
    let b = BenchmarkRun {
        name: "b".into(),
        timestamp: 0,
        duration_secs: 1.0,
        success: false,
        metrics: Default::default(),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "b");
    assert!(!back.success);
}

// ---------------------------------------------------------------------------
// PlatformConfig
// ---------------------------------------------------------------------------

#[test]
fn platform_config_field_assignment() {
    let p = PlatformConfig {
        is_windows: false,
        path_sep: "/".into(),
        shell: "sh".into(),
    };
    assert!(!p.is_windows);
    assert_eq!(p.path_sep, "/");
    assert_eq!(p.shell, "sh");
}

#[test]
fn platform_config_serde() {
    let p = PlatformConfig {
        is_windows: true,
        path_sep: "\\".into(),
        shell: "cmd".into(),
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: PlatformConfig = serde_json::from_str(&j).unwrap();
    assert!(back.is_windows);
}

// ---------------------------------------------------------------------------
// WorkMode
// ---------------------------------------------------------------------------

#[test]
fn work_mode_default_is_standard() {
    let m: WorkMode = Default::default();
    assert_eq!(format!("{m:?}"), "Standard");
}

#[test]
fn work_mode_serde() {
    for m in [WorkMode::Standard, WorkMode::UltraWork] {
        let j = serde_json::to_string(&m).unwrap();
        let back: WorkMode = serde_json::from_str(&j).unwrap();
        assert_eq!(format!("{back:?}"), format!("{m:?}"));
    }
}

// ---------------------------------------------------------------------------
// CustomToolDefinition
// ---------------------------------------------------------------------------

#[test]
fn custom_tool_definition_field_assignment() {
    let t = CustomToolDefinition {
        name: "my_tool".into(),
        description: "d".into(),
        handler: PathBuf::from("/path/to/handler.wasm"),
        schema: serde_json::json!({}),
        fuel_limit: 1000,
    };
    assert_eq!(t.name, "my_tool");
    assert_eq!(t.fuel_limit, 1000);
}

#[test]
fn custom_tool_definition_serde() {
    let t = CustomToolDefinition {
        name: "t".into(),
        description: "d".into(),
        handler: PathBuf::from("/h"),
        schema: serde_json::json!({"type": "object"}),
        fuel_limit: 500,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: CustomToolDefinition = serde_json::from_str(&j).unwrap();
    assert_eq!(back.fuel_limit, 500);
}

// ---------------------------------------------------------------------------
// TriggerDef
// ---------------------------------------------------------------------------

#[test]
fn trigger_def_field_assignment() {
    let t = TriggerDef {
        pattern: Some("deploy".into()),
        tools: vec!["deploy".into()],
        description: Some("deploy hook".into()),
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    assert_eq!(t.pattern.as_deref(), Some("deploy"));
    assert_eq!(t.tools.len(), 1);
}

#[test]
fn trigger_def_serde() {
    let t = TriggerDef {
        pattern: None,
        tools: vec![],
        description: Some("d".into()),
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description.as_deref(), Some("d"));
}
