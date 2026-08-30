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

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_long_tags() {
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: (0..100).map(|i| format!("tag-{}", i)).collect(),
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags.len(), 100);
    assert_eq!(back.tags[42], "tag-42");
}

#[test]
fn plugin_descriptor_with_unicode_checksum() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "插件".into(),
        version: "1.0".into(),
        wasm_path: PathBuf::from("/path/插件.wasm"),
        tool_id: "工具".into(),
        fuel_limit: 5000,
        checksum: Some("哈希-α".into()),
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "插件");
    assert_eq!(back.checksum.as_deref(), Some("哈希-α"));
}

#[test]
fn vector_record_with_long_embedding() {
    use sisyphus_types::VectorRecord;
    let emb: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
    let v = VectorRecord {
        key: "k".into(),
        embedding: emb,
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    assert_eq!(v.embedding.len(), 1000);
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.embedding.len(), 1000);
    assert!((back.embedding[500] - 0.5).abs() < 1e-3);
}

#[test]
fn agent_message_with_unicode_payload() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "发-α".into(),
        to: "收-β".into(),
        kind: "请求".into(),
        payload: serde_json::json!({"内容": "数据-γ"}),
        correlation_id: "关联-1".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from, "发-α");
    assert_eq!(back.payload["内容"], "数据-γ");
}

#[test]
fn benchmark_run_with_max_metrics() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: u64::MAX,
        duration_secs: f64::MAX,
        success: true,
        metrics: serde_json::json!({"x": 1.0e308}),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.timestamp, u64::MAX);
}

#[test]
fn platform_config_debug() {
    use sisyphus_types::PlatformConfig;
    let p = PlatformConfig::default();
    let _ = format!("{:?}", p);
}

#[test]
fn circuit_state_equality() {
    use sisyphus_types::CircuitState;
    assert_eq!(CircuitState::Closed, CircuitState::Closed);
    assert_ne!(CircuitState::Open, CircuitState::Closed);
    assert_ne!(CircuitState::HalfOpen, CircuitState::Open);
}

#[test]
fn circuit_state_debug() {
    use sisyphus_types::CircuitState;
    let _ = format!("{:?}", CircuitState::Closed);
    let _ = format!("{:?}", CircuitState::HalfOpen);
}

#[test]
fn work_mode_ultrawork() {
    use sisyphus_types::WorkMode;
    let wm = WorkMode::UltraWork;
    let j = serde_json::to_string(&wm).unwrap();
    let back: WorkMode = serde_json::from_str(&j).unwrap();
    assert_eq!(back, WorkMode::UltraWork);
}

#[test]
fn custom_tool_definition_default_fuel() {
    use sisyphus_types::CustomToolDefinition;
    let t = CustomToolDefinition {
        name: "n".into(),
        description: "d".into(),
        handler: PathBuf::from("/h"),
        schema: serde_json::json!({}),
        fuel_limit: 1_000_000, // default
    };
    assert_eq!(t.fuel_limit, 1_000_000);
}

#[test]
fn trigger_def_with_many_tools() {
    use sisyphus_types::TriggerDef;
    let tools: Vec<String> = (0..50).map(|i| format!("tool-{}", i)).collect();
    let t = TriggerDef {
        pattern: Some("on_event".into()),
        tools: tools.clone(),
        description: Some("multi-tool trigger".into()),
        hook_phase: Some("after".into()),
        hook_effect: Some("execute".into()),
        hook_message: Some("event".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools.len(), 50);
}

// ---------------------------------------------------------------------------
// Additional AgentMessage coverage
// ---------------------------------------------------------------------------

#[test]
fn agent_message_with_unicode_correlation_id() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "alice".into(),
        to: "bob".into(),
        kind: "request".into(),
        payload: serde_json::json!({}),
        correlation_id: "关联-α".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.correlation_id, "关联-α");
}

#[test]
fn agent_message_with_nested_payload() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "data": {
                "nested": {
                    "deep": [1, 2, 3]
                }
            }
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["data"]["nested"]["deep"][1], 2);
}

#[test]
fn benchmark_run_with_failed_status() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "test".into(),
        timestamp: 0,
        duration_secs: 1.0,
        success: false,
        metrics: serde_json::json!({"error": "OOM"}),
    };
    assert!(!b.success);
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert!(!back.success);
    assert_eq!(back.metrics["error"], "OOM");
}

#[test]
fn benchmark_run_clone() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 1.5,
        success: true,
        metrics: serde_json::json!({}),
    };
    let cloned = b.clone();
    assert_eq!(cloned.name, "n");
    assert_eq!(cloned.duration_secs, 1.5);
}

#[test]
fn platform_config_with_windows_path_sep() {
    use sisyphus_types::PlatformConfig;
    let p = PlatformConfig {
        is_windows: true,
        path_sep: "\\".into(),
        shell: "cmd.exe".into(),
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: PlatformConfig = serde_json::from_str(&j).unwrap();
    assert!(back.is_windows);
    assert_eq!(back.path_sep, "\\");
    assert_eq!(back.shell, "cmd.exe");
}

#[test]
fn platform_config_with_linux_settings() {
    use sisyphus_types::PlatformConfig;
    let p = PlatformConfig {
        is_windows: false,
        path_sep: "/".into(),
        shell: "/bin/zsh".into(),
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: PlatformConfig = serde_json::from_str(&j).unwrap();
    assert!(!back.is_windows);
    assert_eq!(back.shell, "/bin/zsh");
}

#[test]
fn trigger_def_with_unicode_tools() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools: vec!["工具-α".into(), "工具-β".into()],
        description: Some("d".into()),
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools[0], "工具-α");
    assert_eq!(back.tools[1], "工具-β");
}

// ---------------------------------------------------------------------------
// Additional CircuitState and WorkMode coverage
// ---------------------------------------------------------------------------

#[test]
fn circuit_state_serde_round_trip() {
    use sisyphus_types::CircuitState;
    for v in [CircuitState::Closed, CircuitState::Open, CircuitState::HalfOpen] {
        let j = serde_json::to_string(&v).unwrap();
        let back: CircuitState = serde_json::from_str(&j).unwrap();
        assert_eq!(back, v);
    }
}

#[test]
fn circuit_state_distinct_variants() {
    use sisyphus_types::CircuitState;
    let variants = [CircuitState::Closed, CircuitState::Open, CircuitState::HalfOpen];
    for (i, a) in variants.iter().enumerate() {
        for (j, b) in variants.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

#[test]
fn work_mode_standard_is_default() {
    use sisyphus_types::WorkMode;
    let wm: WorkMode = Default::default();
    assert_eq!(wm, WorkMode::Standard);
}

#[test]
fn work_mode_inequality() {
    use sisyphus_types::WorkMode;
    assert_ne!(WorkMode::Standard, WorkMode::UltraWork);
}

#[test]
fn work_mode_clone() {
    use sisyphus_types::WorkMode;
    let wm = WorkMode::UltraWork;
    let cloned = wm.clone();
    assert_eq!(cloned, WorkMode::UltraWork);
}

#[test]
fn trigger_def_with_unicode_pattern_only() {
    use sisyphus_types::TriggerDef;
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"模式-α"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("模式-α"));
    assert!(t.tools.is_empty());
}

#[test]
fn trigger_def_clone_preserves_pattern() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("deploy".into()),
        tools: vec!["t".into()],
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let cloned = t.clone();
    assert_eq!(cloned.pattern, t.pattern);
    assert_eq!(cloned.tools, t.tools);
}

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn circuit_state_default_is_closed() {
    use sisyphus_types::CircuitState;
    let default: CircuitState = serde_json::from_str("\"Closed\"").unwrap();
    assert_eq!(default, CircuitState::Closed);
}

#[test]
fn circuit_state_serde_all_three() {
    use sisyphus_types::CircuitState;
    for state in [CircuitState::Closed, CircuitState::Open, CircuitState::HalfOpen] {
        let j = serde_json::to_string(&state).unwrap();
        let back: CircuitState = serde_json::from_str(&j).unwrap();
        assert_eq!(back, state);
    }
}

#[test]
fn work_mode_equality_of_same_variant() {
    use sisyphus_types::WorkMode;
    assert_eq!(WorkMode::Standard, WorkMode::Standard);
    assert_eq!(WorkMode::UltraWork, WorkMode::UltraWork);
}

#[test]
fn trigger_def_clone_preserves_all() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools: vec!["t1".into(), "t2".into()],
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("validate".into()),
        hook_message: Some("user triggered".into()),
    };
    let cloned = t.clone();
    assert_eq!(cloned.pattern, t.pattern);
    assert_eq!(cloned.tools, t.tools);
    assert_eq!(cloned.description, t.description);
    assert_eq!(cloned.hook_phase, t.hook_phase);
    assert_eq!(cloned.hook_effect, t.hook_effect);
    assert_eq!(cloned.hook_message, t.hook_message);
}

#[test]
fn trigger_def_with_only_hook_effect() {
    use sisyphus_types::TriggerDef;
    let t: TriggerDef = serde_json::from_str(r#"{"hook_effect":"log"}"#).unwrap();
    assert_eq!(t.hook_effect.as_deref(), Some("log"));
    assert!(t.pattern.is_none());
    assert!(t.tools.is_empty());
}

#[test]
fn feedback_with_unicode_request() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "agent-α".into(),
        original_request: "中文请求-β".into(),
        original_output: "输出-γ".into(),
        corrected_output: "修正-δ".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.original_request, "中文请求-β");
    assert_eq!(back.corrected_output, "修正-δ");
}

#[test]
fn feedback_clone_preserves_unicode() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "α".into(),
        original_request: "β".into(),
        original_output: "γ".into(),
        corrected_output: "δ".into(),
        timestamp: 0,
        tags: vec!["标签-1".into()],
    };
    let cloned = f.clone();
    assert_eq!(cloned.agent_id, "α");
    assert_eq!(cloned.tags[0], "标签-1");
}

// ---------------------------------------------------------------------------
// Additional VectorRecord tests
// ---------------------------------------------------------------------------

#[test]
fn vector_record_with_empty_key() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.key, "");
}

#[test]
fn vector_record_with_negative_floats() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![-0.1, -0.5, -1.0],
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.embedding, vec![-0.1, -0.5, -1.0]);
}

#[test]
fn plugin_descriptor_with_relative_wasm_path() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("./target/wasm32/release/plugin.wasm"),
        tool_id: "t".into(),
        fuel_limit: 100,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(
        back.wasm_path,
        std::path::PathBuf::from("./target/wasm32/release/plugin.wasm")
    );
}

#[test]
fn plugin_descriptor_with_long_checksum() {
    use sisyphus_types::PluginDescriptor;
    let long_hash = "a".repeat(128); // SHA-512 hex length
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 100,
        checksum: Some(long_hash.clone()),
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.checksum.as_deref(), Some(long_hash.as_str()));
    assert_eq!(back.checksum.unwrap().len(), 128);
}

#[test]
fn trigger_def_with_only_hook_message() {
    use sisyphus_types::TriggerDef;
    let t: TriggerDef = serde_json::from_str(r#"{"hook_message":"hello"}"#).unwrap();
    assert_eq!(t.hook_message.as_deref(), Some("hello"));
    assert!(t.pattern.is_none());
    assert!(t.hook_phase.is_none());
}

#[test]
fn trigger_def_with_only_hook_phase() {
    use sisyphus_types::TriggerDef;
    let t: TriggerDef = serde_json::from_str(r#"{"hook_phase":"after"}"#).unwrap();
    assert_eq!(t.hook_phase.as_deref(), Some("after"));
    assert!(t.pattern.is_none());
    assert!(t.hook_message.is_none());
}

#[test]
fn trigger_def_with_empty_tools_array() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools: vec![],
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert!(back.tools.is_empty());
}

// ---------------------------------------------------------------------------
// Additional tests for VectorRecord and AgentMessage
// ---------------------------------------------------------------------------

#[test]
fn vector_record_with_special_floats() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.0, 1.0, -1.0, f32::MIN_POSITIVE],
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.embedding.len(), 4);
    assert_eq!(back.embedding[0], 0.0);
    assert_eq!(back.embedding[2], -1.0);
}

#[test]
fn vector_record_with_large_embedding() {
    use sisyphus_types::VectorRecord;
    let emb: Vec<f32> = (0..10000).map(|i| (i as f32) / 10000.0).collect();
    let v = VectorRecord {
        key: "k".into(),
        embedding: emb,
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.embedding.len(), 10000);
}

#[test]
fn agent_message_with_long_correlation_id() {
    use sisyphus_types::AgentMessage;
    let long_id = "x".repeat(1000);
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({}),
        correlation_id: long_id.clone(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.correlation_id.len(), 1000);
}

#[test]
fn feedback_with_negative_timestamp_serde() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: -1000000,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.timestamp, -1000000);
}

#[test]
fn feedback_clone_preserves_timestamp() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 42,
        tags: vec!["t".into()],
    };
    let cloned = f.clone();
    assert_eq!(cloned.timestamp, 42);
    assert_eq!(cloned.tags, vec!["t"]);
}

#[test]
fn plugin_descriptor_with_unicode_version() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1.0-α".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.version, "1.0-α");
}

// ---------------------------------------------------------------------------
// Additional PluginDescriptor and TriggerDef tests
// ---------------------------------------------------------------------------

#[test]
fn plugin_descriptor_with_unicode_in_all_fields() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "插件-α".into(),
        version: "版本-β".into(),
        wasm_path: std::path::PathBuf::from("/路径/插件.wasm"),
        tool_id: "工具-γ".into(),
        fuel_limit: 1000,
        checksum: Some("校验-δ".into()),
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "插件-α");
    assert_eq!(back.version, "版本-β");
    assert_eq!(back.tool_id, "工具-γ");
    assert_eq!(back.checksum.as_deref(), Some("校验-δ"));
}

#[test]
fn trigger_def_with_all_hooks_unicode() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("模式-α".into()),
        tools: vec!["工具-β".into()],
        description: Some("描述-γ".into()),
        hook_phase: Some("阶段-δ".into()),
        hook_effect: Some("效果-ε".into()),
        hook_message: Some("消息-ζ".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.pattern.as_deref(), Some("模式-α"));
    assert_eq!(back.hook_phase.as_deref(), Some("阶段-δ"));
    assert_eq!(back.hook_effect.as_deref(), Some("效果-ε"));
    assert_eq!(back.hook_message.as_deref(), Some("消息-ζ"));
}

#[test]
fn feedback_with_unicode_tags() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec!["标签-α".into(), "标签-β".into(), "标签-γ".into()],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags.len(), 3);
    assert_eq!(back.tags[0], "标签-α");
}

#[test]
fn vector_record_with_only_zero_embedding() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.0; 100],
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.embedding.len(), 100);
    assert!(back.embedding.iter().all(|&x| x == 0.0));
}

#[test]
fn agent_message_with_max_payload_depth() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({"l1": {"l2": {"l3": {"l4": "deep"}}}}),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["l1"]["l2"]["l3"]["l4"], "deep");
}

#[test]
fn benchmark_run_with_complex_metrics() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "stats": {"mean": 0.5, "std": 0.1},
            "list": [1, 2, 3],
            "null": null,
            "bool": true
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["stats"]["mean"], 0.5);
    assert_eq!(back.metrics["list"][2], 3);
    assert_eq!(back.metrics["bool"], true);
}
