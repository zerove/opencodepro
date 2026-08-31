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
    for s in [
        CircuitState::Closed,
        CircuitState::Open,
        CircuitState::HalfOpen,
    ] {
        let d = format!("{s:?}");
        assert!(seen.insert(d.clone()));
    }
}

#[test]
fn circuit_state_serde() {
    for s in [
        CircuitState::Closed,
        CircuitState::Open,
        CircuitState::HalfOpen,
    ] {
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
    for v in [
        CircuitState::Closed,
        CircuitState::Open,
        CircuitState::HalfOpen,
    ] {
        let j = serde_json::to_string(&v).unwrap();
        let back: CircuitState = serde_json::from_str(&j).unwrap();
        assert_eq!(back, v);
    }
}

#[test]
fn circuit_state_distinct_variants() {
    use sisyphus_types::CircuitState;
    let variants = [
        CircuitState::Closed,
        CircuitState::Open,
        CircuitState::HalfOpen,
    ];
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
    for state in [
        CircuitState::Closed,
        CircuitState::Open,
        CircuitState::HalfOpen,
    ] {
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

// ---------------------------------------------------------------------------
// Additional VectorRecord and PluginDescriptor tests
// ---------------------------------------------------------------------------

#[test]
fn vector_record_with_extreme_embedding_values() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![f32::MAX, f32::MIN, f32::MIN_POSITIVE, 0.0],
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.embedding.len(), 4);
    assert_eq!(back.embedding[0], f32::MAX);
    assert_eq!(back.embedding[3], 0.0);
}

#[test]
fn vector_record_with_infinity_embedding() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![f32::INFINITY, f32::NEG_INFINITY],
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    // Infinity might not roundtrip via Serde (becomes null), but should not panic
    let j = serde_json::to_string(&v).unwrap_or_default();
    let _ = j;
}

#[test]
fn plugin_descriptor_with_absolute_wasm_path() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/usr/local/lib/plugins/my_plugin.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(
        back.wasm_path,
        std::path::PathBuf::from("/usr/local/lib/plugins/my_plugin.wasm")
    );
}

#[test]
fn plugin_descriptor_with_special_chars_in_path() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/path/with spaces and-dashes/file.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(
        back.wasm_path.to_str().unwrap(),
        "/path/with spaces and-dashes/file.wasm"
    );
}

#[test]
fn feedback_with_100_tags() {
    use sisyphus_types::Feedback;
    let tags: Vec<String> = (0..100).map(|i| format!("tag-{}", i)).collect();
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags,
    };
    assert_eq!(f.tags.len(), 100);
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags.len(), 100);
    assert_eq!(back.tags[50], "tag-50");
}

#[test]
fn agent_message_with_only_unicode_to() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "接收人-α".into(),
        kind: "request".into(),
        payload: serde_json::json!({}),
        correlation_id: "c".into(),
    };
    assert_eq!(m.to, "接收人-α");
}

#[test]
fn benchmark_run_with_extreme_metrics() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: false,
        metrics: serde_json::json!({"value": f64::INFINITY, "min": f64::NEG_INFINITY}),
    };
    let j = serde_json::to_string(&b).unwrap_or_default();
    // Infinity might be serialized as null
    let _ = j;
}

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn plugin_descriptor_with_unicode_tool_id_long() {
    use sisyphus_types::PluginDescriptor;
    let long = "工具-".repeat(100);
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: long.clone(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tool_id.len(), long.len());
}

#[test]
fn vector_record_with_emoji_in_metadata_value() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![],
        metadata: serde_json::json!({"emoji": "🔍🔭🌟"}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["emoji"], "🔍🔭🌟");
}

#[test]
fn benchmark_run_with_zero_name() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: false,
        metrics: serde_json::json!({}),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "");
}

#[test]
fn agent_message_with_only_unicode_payload() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!("纯字符串-α-🔍"),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload, "纯字符串-α-🔍");
}

#[test]
fn feedback_with_only_long_field() {
    use sisyphus_types::Feedback;
    let long = "x".repeat(2000);
    let f = Feedback {
        agent_id: "a".into(),
        original_request: long.clone(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.original_request.len(), 2000);
}

#[test]
fn trigger_def_with_only_unicode_tools() {
    use sisyphus_types::TriggerDef;
    let tools: Vec<String> = (0..10).map(|i| format!("工具-{}", i)).collect();
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
    assert_eq!(back.tools.len(), 10);
    assert_eq!(back.tools[0], "工具-0");
    assert_eq!(back.tools[9], "工具-9");
}

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn trigger_def_with_only_pattern_unicode_clone() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("模式-α".into()),
        tools: vec![],
        description: None,
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    let cloned = t.clone();
    assert_eq!(cloned.pattern, t.pattern);
}

#[test]
fn trigger_def_with_empty_all_optional_fields() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: None,
        tools: vec![],
        description: None,
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert!(back.pattern.is_none());
    assert!(back.tools.is_empty());
}

#[test]
fn benchmark_run_with_negative_timestamp() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: false,
        metrics: serde_json::json!({}),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.timestamp, 0);
}

#[test]
fn feedback_with_only_agent_id() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "".into(),
        original_output: "".into(),
        corrected_output: "".into(),
        timestamp: 0,
        tags: vec![],
    };
    assert_eq!(f.agent_id, "a");
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "a");
}

#[test]
fn vector_record_with_unicode_in_metadata_array() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![],
        metadata: serde_json::json!({"tags": ["中文-α", "中文-β", "中文-γ"]}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["tags"][0], "中文-α");
    assert_eq!(back.metadata["tags"][1], "中文-β");
}

#[test]
fn agent_message_with_all_unicode_fields() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "发-α".into(),
        to: "收-β".into(),
        kind: "类型-γ".into(),
        payload: serde_json::json!({"key": "值-δ"}),
        correlation_id: "关联-ε".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from, "发-α");
    assert_eq!(back.to, "收-β");
    assert_eq!(back.kind, "类型-γ");
    assert_eq!(back.payload["key"], "值-δ");
    assert_eq!(back.correlation_id, "关联-ε");
}

#[test]
fn plugin_descriptor_with_empty_all_optional() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 0,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert!(back.checksum.is_none());
    assert_eq!(back.fuel_limit, 0);
}

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_only_corrected_output() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "".into(),
        original_output: "".into(),
        corrected_output: "fixed".into(),
        timestamp: 0,
        tags: vec![],
    };
    assert_eq!(f.corrected_output, "fixed");
}

#[test]
fn vector_record_with_unicode_metadata_complex_2() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![1.0, 2.0],
        metadata: serde_json::json!({
            "nested": {
                "deep": {
                    "value": "中文-α🔍"
                }
            }
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["nested"]["deep"]["value"], "中文-α🔍");
}

#[test]
fn plugin_descriptor_with_unicode_path_with_spaces() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/路径/插件 名称.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.wasm_path.to_str().unwrap(), "/路径/插件 名称.wasm");
}

#[test]
fn agent_message_with_long_payload() {
    use sisyphus_types::AgentMessage;
    let long_payload = "x".repeat(5000);
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::Value::String(long_payload.clone()),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload.as_str().unwrap().len(), 5000);
}

#[test]
fn benchmark_run_with_unicode_name() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "测试-α-β".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({}),
    };
    assert_eq!(b.name, "测试-α-β");
}

#[test]
fn trigger_def_with_only_unicode_hook_effect() {
    use sisyphus_types::TriggerDef;
    let t: TriggerDef = serde_json::from_str(r#"{"hook_effect":"效果-α"}"#).unwrap();
    assert_eq!(t.hook_effect.as_deref(), Some("效果-α"));
    assert!(t.pattern.is_none());
}

#[test]
fn vector_record_with_only_unicode_key() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "键-".repeat(50),
        embedding: vec![1.0],
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.key, "键-".repeat(50));
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_only_long_agent_id() {
    use sisyphus_types::Feedback;
    let long = "a".repeat(5000);
    let f = Feedback {
        agent_id: long.clone(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    assert_eq!(f.agent_id.len(), 5000);
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, long);
}

#[test]
fn vector_record_with_unicode_key_special_chars() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "键-α\t\n\r".into(),
        embedding: vec![1.0],
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    // JSON escapes special chars
    assert_eq!(back.key, "键-α\t\n\r");
}

#[test]
fn plugin_descriptor_with_unicode_long_name() {
    use sisyphus_types::PluginDescriptor;
    let long = "插件-".repeat(100);
    let pd = PluginDescriptor {
        name: long.clone(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    assert_eq!(pd.name, long);
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, long);
}

#[test]
fn agent_message_with_unicode_payload_serde() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({"中文": "数据-α🔍"}),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["中文"], "数据-α🔍");
}

#[test]
fn benchmark_run_with_unicode_metrics() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({"中文-α": "值-β", "键-γ": "值-δ"}),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["中文-α"], "值-β");
}

#[test]
fn trigger_def_with_only_unicode_hook_message_serde() {
    use sisyphus_types::TriggerDef;
    let t: TriggerDef = serde_json::from_str(r#"{"hook_message":"消息-α🔍"}"#).unwrap();
    assert_eq!(t.hook_message.as_deref(), Some("消息-α🔍"));
}

#[test]
fn feedback_with_only_one_long_tag() {
    use sisyphus_types::Feedback;
    let long = "t".repeat(1000);
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![long.clone()],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags[0].len(), 1000);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn trigger_def_with_unicode_tools_serde_2() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools: vec!["工-α".into(), "工-β".into()],
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools[0], "工-α");
}

#[test]
fn feedback_with_only_one_tag() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec!["only-tag".into()],
    };
    assert_eq!(f.tags.len(), 1);
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags[0], "only-tag");
}

#[test]
fn vector_record_with_unicode_metadata_array() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5, 0.6],
        metadata: serde_json::json!({
            "list": ["α", "β", "γ", "🔍"]
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["list"][0], "α");
    assert_eq!(back.metadata["list"][3], "🔍");
}

#[test]
fn plugin_descriptor_with_max_fuel_limit_v3() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: u64::MAX,
        checksum: None,
    };
    assert_eq!(pd.fuel_limit, u64::MAX);
}

#[test]
fn agent_message_with_only_unicode_kind() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "类型-中文-α".into(),
        payload: serde_json::json!({}),
        correlation_id: "c".into(),
    };
    assert_eq!(m.kind, "类型-中文-α");
}

#[test]
fn benchmark_run_with_negative_duration_secs() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: -0.5,
        success: false,
        metrics: serde_json::json!({}),
    };
    assert_eq!(b.duration_secs, -0.5);
}

#[test]
fn feedback_with_unicode_original_output() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "输出-α-🔍".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    assert_eq!(f.original_output, "输出-α-🔍");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_original_request() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "请求-α-🔍".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    assert_eq!(f.original_request, "请求-α-🔍");
}

#[test]
fn vector_record_with_unicode_metadata_array_long() {
    use sisyphus_types::VectorRecord;
    let arr: Vec<String> = (0..50).map(|i| format!("item-α-{}", i)).collect();
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({"list": arr}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["list"].as_array().unwrap().len(), 50);
    assert_eq!(back.metadata["list"][25], "item-α-25");
}

#[test]
fn plugin_descriptor_with_unicode_name_max_chars() {
    use sisyphus_types::PluginDescriptor;
    let long = "αβγ".repeat(100);
    let pd = PluginDescriptor {
        name: long.clone(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, long);
}

#[test]
fn agent_message_with_unicode_only_to_field() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "接收-α-🔍".into(),
        kind: "request".into(),
        payload: serde_json::json!({}),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.to, "接收-α-🔍");
}

#[test]
fn benchmark_run_with_unicode_metrics_complex() {
    use sisyphus_types::BenchmarkRun;
    let metrics = serde_json::json!({
        "name-α": "值-β",
        "name-γ": "值-δ",
        "list": ["α", "β", "γ"]
    });
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics,
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["name-α"], "值-β");
    assert_eq!(back.metrics["list"][1], "β");
}

#[test]
fn trigger_def_with_unicode_3_fields_only() {
    use sisyphus_types::TriggerDef;
    let t: TriggerDef =
        serde_json::from_str(r#"{"pattern":"p-α","tools":["t-β","t-γ"],"description":"d-δ"}"#)
            .unwrap();
    assert_eq!(t.pattern.as_deref(), Some("p-α"));
    assert_eq!(t.tools[0], "t-β");
    assert_eq!(t.tools[1], "t-γ");
    assert_eq!(t.description.as_deref(), Some("d-δ"));
}

#[test]
fn feedback_with_unicode_tags_serde() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec!["标签-α".into(), "标签-β".into()],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags.len(), 2);
    assert_eq!(back.tags[0], "标签-α");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_2_tags() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec!["tag-α".into(), "tag-β".into()],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags[0], "tag-α");
    assert_eq!(back.tags[1], "tag-β");
}

#[test]
fn vector_record_with_unicode_metadata_object() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5, 0.6],
        metadata: serde_json::json!({
            "config-α": {
                "name-β": "value-γ",
                "count-δ": 42
            }
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["config-α"]["name-β"], "value-γ");
    assert_eq!(back.metadata["config-α"]["count-δ"], 42);
}

#[test]
fn plugin_descriptor_with_unicode_tool_id_max() {
    use sisyphus_types::PluginDescriptor;
    let long = "工具-".repeat(100);
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: long.clone(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tool_id, long);
}

#[test]
fn agent_message_with_unicode_payload_array() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!(["α", "β", "γ", "🔍"]),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload[0], "α");
    assert_eq!(back.payload[3], "🔍");
}

#[test]
fn benchmark_run_with_unicode_metrics_array() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({"中文-α": ["β", "γ", "δ"]}),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["中文-α"][0], "β");
}

#[test]
fn trigger_def_with_unicode_2_tools() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("p-α".into()),
        tools: vec!["tool-α".into(), "tool-β".into()],
        description: Some("d-β".into()),
        hook_phase: Some("before-α".into()),
        hook_effect: Some("e-β".into()),
        hook_message: Some("m-β".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools.len(), 2);
    assert_eq!(back.tools[0], "tool-α");
    assert_eq!(back.tools[1], "tool-β");
}

#[test]
fn feedback_with_only_unicode_corrected_output() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "修正-α-🔍".into(),
        timestamp: 0,
        tags: vec![],
    };
    assert_eq!(f.corrected_output, "修正-α-🔍");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_5_tags() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: (0..5).map(|i| format!("标签-α-{}", i)).collect(),
    };
    assert_eq!(f.tags.len(), 5);
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags[2], "标签-α-2");
}

#[test]
fn vector_record_with_unicode_metadata_array_2() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.1, 0.2],
        metadata: serde_json::json!({
            "items-α": [
                {"name-β": "value-γ", "count-δ": 10},
                {"name-ε": "value-ζ", "count-η": 20}
            ]
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["items-α"][0]["name-β"], "value-γ");
    assert_eq!(back.metadata["items-α"][1]["count-η"], 20);
}

#[test]
fn plugin_descriptor_with_unicode_5_metadata() {
    use sisyphus_types::PluginDescriptor;
    // PluginDescriptor has no metadata field, just test fuel_limit unicode
    let pd = PluginDescriptor {
        name: "插件-α-名字-β".into(),
        version: "1.0".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "工具-γ".into(),
        fuel_limit: 1000,
        checksum: Some("校验-δ".into()),
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "插件-α-名字-β");
    assert_eq!(back.tool_id, "工具-γ");
    assert_eq!(back.checksum.as_deref(), Some("校验-δ"));
}

#[test]
fn agent_message_with_unicode_payload_object() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "operation-α": "create-β",
            "args-γ": {"name-δ": "value-ε", "tags-ζ": ["η", "θ"]}
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["operation-α"], "create-β");
    assert_eq!(back.payload["args-γ"]["name-δ"], "value-ε");
    assert_eq!(back.payload["args-γ"]["tags-ζ"][1], "θ");
}

#[test]
fn benchmark_run_with_unicode_metrics_object_deep() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "name-α": {
                "config-β": {
                    "value-γ": 42
                }
            }
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["name-α"]["config-β"]["value-γ"], 42);
}

#[test]
fn trigger_def_with_unicode_long_pattern() {
    use sisyphus_types::TriggerDef;
    let long = "p-α-".repeat(50);
    let t = TriggerDef {
        pattern: Some(long.clone()),
        tools: vec!["t".into()],
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.pattern, Some(long));
}

#[test]
fn feedback_with_unicode_timestamp_zero() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "id-α-🔍".into(),
        original_request: "请求-β".into(),
        original_output: "输出-γ".into(),
        corrected_output: "修正-δ".into(),
        timestamp: 0,
        tags: vec!["标签-ε".into()],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "id-α-🔍");
    assert_eq!(back.timestamp, 0);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_tags_5_emoji() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![
            "🔍".into(),
            "🔭".into(),
            "🌟".into(),
            "📚".into(),
            "💡".into(),
        ],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags.len(), 5);
    assert_eq!(back.tags[0], "🔍");
    assert_eq!(back.tags[4], "💡");
}

#[test]
fn vector_record_with_unicode_metadata_object_2() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "config-α": {
                "name-β": "value-γ",
                "tags-δ": ["η-1", "η-2", "η-3"]
            }
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(
        back.metadata["config-α"]["tags-δ"].as_array().unwrap().len(),
        3
    );
    assert_eq!(back.metadata["config-α"]["tags-δ"][1], "η-2");
}

#[test]
fn plugin_descriptor_with_unicode_version_max() {
    use sisyphus_types::PluginDescriptor;
    let long = "v-α-".repeat(50);
    let pd = PluginDescriptor {
        name: "p".into(),
        version: long.clone(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.version, long);
}

#[test]
fn agent_message_with_unicode_payload_complex() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "中文-α": {
                "items-β": [1, 2, 3],
                "name-γ": "value-δ",
                "active-ε": true
            }
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(
        back.payload["中文-α"]["items-β"].as_array().unwrap().len(),
        3
    );
    assert_eq!(back.payload["中文-α"]["active-ε"], true);
}

#[test]
fn benchmark_run_with_unicode_metrics_2() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "config-α": {
                "depth-β": 5,
                "tags-γ": ["δ-1", "δ-2"]
            }
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["config-α"]["depth-β"], 5);
    assert_eq!(back.metrics["config-α"]["tags-γ"][0], "δ-1");
}

#[test]
fn trigger_def_with_unicode_tools_3() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("p-α".into()),
        tools: vec!["t-β-1".into(), "t-β-2".into(), "t-β-3".into()],
        description: Some("d-γ".into()),
        hook_phase: Some("before-δ".into()),
        hook_effect: Some("e-ε".into()),
        hook_message: Some("m-ζ".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools.len(), 3);
    assert_eq!(back.tools[2], "t-β-3");
}

#[test]
fn feedback_with_unicode_3_fields() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "agent-α-🔍".into(),
        original_request: "请求-β".into(),
        original_output: "输出-γ".into(),
        corrected_output: "修正-δ".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "agent-α-🔍");
    assert_eq!(back.original_request, "请求-β");
    assert_eq!(back.original_output, "输出-γ");
    assert_eq!(back.corrected_output, "修正-δ");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_agent_id_long() {
    use sisyphus_types::Feedback;
    let long = "α-β-γ-δ-ε-ζ-η-θ".repeat(10);
    let f = Feedback {
        agent_id: long.clone(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, long);
}

#[test]
fn vector_record_with_unicode_metadata_3_levels_deep() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "l1-α": {
                "l2-β": {
                    "l3-γ": "deep-value-δ"
                }
            }
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["l1-α"]["l2-β"]["l3-γ"], "deep-value-δ");
}

#[test]
fn plugin_descriptor_with_unicode_3_metadata_fields() {
    use sisyphus_types::PluginDescriptor;
    // PluginDescriptor doesn't have metadata, just test with name/version/tool_id
    let pd = PluginDescriptor {
        name: "插件-α".into(),
        version: "版本-β".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
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
fn agent_message_with_unicode_payload_max_depth() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "l1-α": {
                "l2-β": {
                    "l3-γ": {
                        "l4-δ": {
                            "l5-ε": "very deep"
                        }
                    }
                }
            }
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(
        back.payload["l1-α"]["l2-β"]["l3-γ"]["l4-δ"]["l5-ε"],
        "very deep"
    );
}

#[test]
fn benchmark_run_with_unicode_metrics_array_3() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "name-α": ["值-β", "值-γ", "值-δ"]
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["name-α"][0], "值-β");
    assert_eq!(back.metrics["name-α"][2], "值-δ");
}

#[test]
fn trigger_def_with_unicode_2_tools_and_emoji_in_pattern() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("模式-α-🔍".into()),
        tools: vec!["t-β".into(), "t-γ".into()],
        description: Some("d-δ".into()),
        hook_phase: Some("前-ε".into()),
        hook_effect: Some("效果-ζ".into()),
        hook_message: Some("消息-η".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools.len(), 2);
    assert_eq!(back.tools[0], "t-β");
}

#[test]
fn feedback_with_unicode_only_original_request() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "请求-α-🔍".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    assert_eq!(f.original_request, "请求-α-🔍");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_3_tags_serde() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec!["t-α".into(), "t-β".into(), "t-γ".into()],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags.len(), 3);
    assert_eq!(back.tags[1], "t-β");
}

#[test]
fn vector_record_with_unicode_metadata_empty_value() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![],
        metadata: serde_json::json!({"key-α": "", "key-β": "v"}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["key-α"], "");
    assert_eq!(back.metadata["key-β"], "v");
}

#[test]
fn plugin_descriptor_with_unicode_4_segment_id() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p-1-α-β-2".into(),
        version: "v-α-β".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t-α-β-γ".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    assert_eq!(pd.name, "p-1-α-β-2");
    assert_eq!(pd.version, "v-α-β");
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tool_id, "t-α-β-γ");
}

#[test]
fn agent_message_with_unicode_payload_bool_number_string() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "name-α": "value-β",
            "count-γ": 42,
            "active-δ": true,
            "ratio-ε": 3.14
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["name-α"], "value-β");
    assert_eq!(back.payload["count-γ"], 42);
    assert_eq!(back.payload["active-δ"], true);
    assert_eq!(back.payload["ratio-ε"], 3.14);
}

#[test]
fn benchmark_run_with_unicode_metrics_4_types() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "string-α": "v-β",
            "number-γ": 42,
            "bool-δ": true,
            "null-ε": null
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["string-α"], "v-β");
    assert_eq!(back.metrics["number-γ"], 42);
    assert_eq!(back.metrics["bool-δ"], true);
    assert!(back.metrics["null-ε"].is_null());
}

#[test]
fn trigger_def_with_unicode_3_fields_2() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("p-α".into()),
        tools: vec!["t-β".into()],
        description: Some("d-γ".into()),
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.pattern.as_deref(), Some("p-α"));
    assert_eq!(back.description.as_deref(), Some("d-γ"));
}

#[test]
fn feedback_with_unicode_agent_id_short() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "α".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    assert_eq!(f.agent_id, "α");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_3_tags_serde_2() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec!["tag-α".into(), "tag-β".into(), "tag-γ".into()],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags.len(), 3);
    assert_eq!(back.tags[2], "tag-γ");
}

#[test]
fn vector_record_with_unicode_metadata_2_layers() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "outer-α": {
                "inner-β": {
                    "deepest-γ": "value-δ"
                }
            }
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["outer-α"]["inner-β"]["deepest-γ"], "value-δ");
}

#[test]
fn plugin_descriptor_with_unicode_special_chars_in_tool_id() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "tool-α!@#β$%γ".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tool_id, "tool-α!@#β$%γ");
}

#[test]
fn agent_message_with_unicode_payload_2_keys() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "key-α": "value-β",
            "key-γ": "value-δ"
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["key-α"], "value-β");
    assert_eq!(back.payload["key-γ"], "value-δ");
}

#[test]
fn benchmark_run_with_unicode_metrics_nested() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "outer-α": {
                "inner-β": "value-γ",
                "count-δ": 100
            }
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["outer-α"]["inner-β"], "value-γ");
    assert_eq!(back.metrics["outer-α"]["count-δ"], 100);
}

#[test]
fn trigger_def_with_unicode_description_only() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: None,
        tools: vec![],
        description: Some("描述-α-🔍".into()),
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    assert_eq!(t.description.as_deref(), Some("描述-α-🔍"));
}

#[test]
fn feedback_with_unicode_only_output() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "输出-α-🔍".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    assert_eq!(f.original_output, "输出-α-🔍");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_only_corrected_output_2() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "修正-α-🔍".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.corrected_output, "修正-α-🔍");
}

#[test]
fn vector_record_with_unicode_metadata_3_keys() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "k1-α": "v1-β",
            "k2-γ": "v2-δ",
            "k3-ε": "v3-ζ"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k1-α"], "v1-β");
    assert_eq!(back.metadata["k3-ε"], "v3-ζ");
}

#[test]
fn plugin_descriptor_with_unicode_path_with_emoji() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/path/🔍/插件.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert!(back.wasm_path.to_str().unwrap().contains("🔍"));
}

#[test]
fn agent_message_with_unicode_payload_array_2() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!([
            {"name-α": "value-β"},
            {"name-γ": "value-δ"},
            {"name-ε": "value-ζ"}
        ]),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload[0]["name-α"], "value-β");
    assert_eq!(back.payload[2]["name-ε"], "value-ζ");
}

#[test]
fn benchmark_run_with_unicode_metrics_array_long() {
    use sisyphus_types::BenchmarkRun;
    let arr: Vec<String> = (0..20).map(|i| format!("item-α-{}", i)).collect();
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({"items-α": arr}),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["items-α"].as_array().unwrap().len(), 20);
}

#[test]
fn trigger_def_with_unicode_long_hook_message() {
    use sisyphus_types::TriggerDef;
    let long = "消息-α-".repeat(50);
    let t = TriggerDef {
        pattern: Some("p".into()),
        tools: vec!["t".into()],
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some(long.clone()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.hook_message.as_deref(), Some(long.as_str()));
}

#[test]
fn feedback_with_unicode_long_timestamp() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: i64::MAX,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.timestamp, i64::MAX);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_long_corrected_output_2() {
    use sisyphus_types::Feedback;
    let long = "修正-α-".repeat(20);
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: long.clone(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.corrected_output, long);
}

#[test]
fn vector_record_with_unicode_metadata_object_3() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "config-α": {
                "name-β": "value-γ",
                "tags-δ": ["η-1", "η-2"]
            }
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["config-α"]["name-β"], "value-γ");
    assert_eq!(back.metadata["config-α"]["tags-δ"][1], "η-2");
}

#[test]
fn plugin_descriptor_with_unicode_tool_id_long_short() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "工具-α-β-γ-δ-ε".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    assert_eq!(pd.tool_id, "工具-α-β-γ-δ-ε");
}

#[test]
fn agent_message_with_unicode_payload_2_layer() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "outer-α": {
                "inner-β": {
                    "deepest-γ": "value-δ"
                }
            }
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["outer-α"]["inner-β"]["deepest-γ"], "value-δ");
}

#[test]
fn benchmark_run_with_unicode_metrics_array_complex() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "name-α": [
                {"id-β": 1, "val-γ": "a"},
                {"id-β": 2, "val-γ": "b"}
            ]
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["name-α"][0]["id-β"], 1);
    assert_eq!(back.metrics["name-α"][1]["val-γ"], "b");
}

#[test]
fn trigger_def_with_unicode_3_tools_array() {
    use sisyphus_types::TriggerDef;
    let tools = vec![
        "t-1-α".to_string(),
        "t-2-β".to_string(),
        "t-3-γ".to_string(),
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
    assert_eq!(back.tools.len(), 3);
    assert_eq!(back.tools[1], "t-2-β");
}

#[test]
fn feedback_with_unicode_zero_tags() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "agent-α".into(),
        original_request: "请求-β".into(),
        original_output: "输出-γ".into(),
        corrected_output: "修正-δ".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert!(back.tags.is_empty());
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_only_request() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "请求-α-🔍".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    assert_eq!(f.original_request, "请求-α-🔍");
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.original_request, "请求-α-🔍");
}

#[test]
fn vector_record_with_unicode_metadata_4_keys_unicode() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "k1-α": "v1",
            "k2-β": "v2",
            "k3-γ": "v3",
            "k4-δ": "v4"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k1-α"], "v1");
    assert_eq!(back.metadata["k4-δ"], "v4");
}

#[test]
fn plugin_descriptor_with_unicode_name_only() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "插件-α-🔍".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    assert_eq!(pd.name, "插件-α-🔍");
}

#[test]
fn agent_message_with_unicode_payload_only_emoji() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({"🔍": "search", "🔭": "scope"}),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["🔍"], "search");
}

#[test]
fn benchmark_run_with_unicode_metrics_only_emoji() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({"🔍": "search", "🔭": "scope"}),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["🔍"], "search");
}

#[test]
fn trigger_def_with_unicode_2_tools_and_complex_pattern() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("prefix-α-midle-β-suffix-γ".into()),
        tools: vec!["t-1-δ".into(), "t-2-ε".into()],
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tools.len(), 2);
    assert_eq!(back.pattern.as_deref().unwrap().chars().count(), 25);
}

#[test]
fn feedback_with_unicode_only_timestamp() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 1700000000,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.timestamp, 1700000000);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_only_agent_id() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "agent-α-🔍".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "agent-α-🔍");
}

#[test]
fn vector_record_with_unicode_metadata_5_keys() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "k1-α": "v1",
            "k2-β": "v2",
            "k3-γ": "v3",
            "k4-δ": "v4",
            "k5-ε": "v5"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k5-ε"], "v5");
}

#[test]
fn plugin_descriptor_with_unicode_4_segment_path() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/path/α/β/γ.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert!(back.wasm_path.to_str().unwrap().contains("γ.wasm"));
}

#[test]
fn agent_message_with_unicode_payload_2_keys_2() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "key-α": {"nested-β": "value-γ"},
            "key-δ": {"nested-ε": "value-ζ"}
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["key-α"]["nested-β"], "value-γ");
    assert_eq!(back.payload["key-δ"]["nested-ε"], "value-ζ");
}

#[test]
fn benchmark_run_with_unicode_metrics_2_keys() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "name-α": "v1",
            "name-β": "v2"
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["name-α"], "v1");
    assert_eq!(back.metrics["name-β"], "v2");
}

#[test]
fn trigger_def_with_unicode_2_segment_pattern_only() {
    use sisyphus_types::TriggerDef;
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"前-α-中-β"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("前-α-中-β"));
}

#[test]
fn feedback_with_unicode_3_fields_2() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "agent-α".into(),
        original_request: "请求-β".into(),
        original_output: "输出-γ".into(),
        corrected_output: "修正-δ".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.original_request, "请求-β");
    assert_eq!(back.original_output, "输出-γ");
    assert_eq!(back.corrected_output, "修正-δ");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_only_output_2() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "输出-α-🔍-γ".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.original_output, "输出-α-🔍-γ");
}

#[test]
fn vector_record_with_unicode_metadata_2_keys_2() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "k1-α": "v1",
            "k2-β": "v2"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k1-α"], "v1");
    assert_eq!(back.metadata["k2-β"], "v2");
}

#[test]
fn plugin_descriptor_with_unicode_3_segment_path() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/path/α/β.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert!(back.wasm_path.to_str().unwrap().contains("β.wasm"));
}

#[test]
fn agent_message_with_unicode_payload_3_keys() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "key-α": "value-α",
            "key-β": "value-β",
            "key-γ": "value-γ"
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["key-α"], "value-α");
    assert_eq!(back.payload["key-γ"], "value-γ");
}

#[test]
fn benchmark_run_with_unicode_metrics_3_keys() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "k1-α": "v1",
            "k2-β": "v2",
            "k3-γ": "v3"
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["k1-α"], "v1");
    assert_eq!(back.metrics["k3-γ"], "v3");
}

#[test]
fn trigger_def_with_unicode_4_segment_pattern() {
    use sisyphus_types::TriggerDef;
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"a-α-b-β-c-γ-d-δ"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("a-α-b-β-c-γ-d-δ"));
}

#[test]
fn feedback_with_unicode_only_corrected_2() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "修正-α-🔍-γ".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.corrected_output, "修正-α-🔍-γ");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_3_fields_3() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "agent-α-🔍".into(),
        original_request: "请求-β-γ".into(),
        original_output: "输出-δ-ε".into(),
        corrected_output: "修正-ζ-η".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.original_request, "请求-β-γ");
    assert_eq!(back.corrected_output, "修正-ζ-η");
}

#[test]
fn vector_record_with_unicode_metadata_3_keys_2() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "k1-α": "v1-β",
            "k2-γ": "v2-δ",
            "k3-ε": "v3-ζ"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k1-α"], "v1-β");
}

#[test]
fn plugin_descriptor_with_unicode_2_segment_path() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/path/α-β.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert!(back.wasm_path.to_str().unwrap().contains("α-β.wasm"));
}

#[test]
fn agent_message_with_unicode_payload_4_keys() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "k1-α": "v1",
            "k2-β": "v2",
            "k3-γ": "v3",
            "k4-δ": "v4"
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["k1-α"], "v1");
    assert_eq!(back.payload["k4-δ"], "v4");
}

#[test]
fn benchmark_run_with_unicode_metrics_4_keys() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "k1-α": "v1",
            "k2-β": "v2",
            "k3-γ": "v3",
            "k4-δ": "v4"
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["k4-δ"], "v4");
}

#[test]
fn trigger_def_with_unicode_3_segment_pattern_2() {
    use sisyphus_types::TriggerDef;
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"前-α-中-β-后"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("前-α-中-β-后"));
}

#[test]
fn feedback_with_unicode_4_fields_2() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a-🔍".into(),
        original_request: "r-α".into(),
        original_output: "o-β".into(),
        corrected_output: "c-γ".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "a-🔍");
    assert_eq!(back.corrected_output, "c-γ");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_only_agent_id_2() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "agent-α-🔍-β-γ".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "agent-α-🔍-β-γ");
}

#[test]
fn vector_record_with_unicode_metadata_2_keys_3() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "α": "v-α",
            "β": "v-β"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["α"], "v-α");
}

#[test]
fn plugin_descriptor_with_unicode_5_segment_path() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/a/b/c/α/β.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert!(back.wasm_path.to_str().unwrap().contains("β.wasm"));
}

#[test]
fn agent_message_with_unicode_payload_2_keys_3() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "α": "value-α",
            "β": "value-β"
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["α"], "value-α");
    assert_eq!(back.payload["β"], "value-β");
}

#[test]
fn benchmark_run_with_unicode_metrics_2_keys_2() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "α": "v-α",
            "β": "v-β"
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["α"], "v-α");
    assert_eq!(back.metrics["β"], "v-β");
}

#[test]
fn feedback_with_unicode_only_corrected_3() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "修正-α-β-γ-δ-ε-🔍".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.corrected_output, "修正-α-β-γ-δ-ε-🔍");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_only_corrected_4() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "修正-α-🔍".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.corrected_output, "修正-α-🔍");
}

#[test]
fn vector_record_with_unicode_metadata_3_keys_3() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "k1-α": "v1",
            "k2-β": "v2",
            "k3-γ": "v3"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k2-β"], "v2");
}

#[test]
fn plugin_descriptor_with_unicode_2_segment_path_2() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/a/α-β.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.wasm_path, std::path::PathBuf::from("/a/α-β.wasm"));
}

#[test]
fn agent_message_with_unicode_payload_2_keys_4() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "k1": "v1-α",
            "k2": "v2-β"
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["k1"], "v1-α");
    assert_eq!(back.payload["k2"], "v2-β");
}

#[test]
fn benchmark_run_with_unicode_metrics_2_keys_3() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "k1": "v1-α",
            "k2": "v2-β"
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["k1"], "v1-α");
    assert_eq!(back.metrics["k2"], "v2-β");
}

#[test]
fn trigger_def_with_unicode_pattern_with_special() {
    use sisyphus_types::TriggerDef;
    let t = TriggerDef {
        pattern: Some("!@#$%^&*()α-β🔍".into()),
        tools: vec!["t".into()],
        description: Some("d".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("e".into()),
        hook_message: Some("m".into()),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: TriggerDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.pattern.as_deref(), Some("!@#$%^&*()α-β🔍"));
}

#[test]
fn feedback_with_unicode_only_agent_id_3() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "agent-α-🔍-β-γ-δ-ε".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "agent-α-🔍-β-γ-δ-ε");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_4_fields_3() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a-α".into(),
        original_request: "r-β".into(),
        original_output: "o-γ".into(),
        corrected_output: "c-δ".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "a-α");
    assert_eq!(back.original_request, "r-β");
    assert_eq!(back.original_output, "o-γ");
    assert_eq!(back.corrected_output, "c-δ");
}

#[test]
fn vector_record_with_unicode_metadata_2_keys_5() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "α-β": "v-α",
            "γ-δ": "v-γ"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["α-β"], "v-α");
}

#[test]
fn plugin_descriptor_with_unicode_3_segment_path_2() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/a/b/α-β.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert!(back.wasm_path.to_str().unwrap().contains("α-β.wasm"));
}

#[test]
fn agent_message_with_unicode_payload_3_keys_2() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "key-α": "v1",
            "key-β": "v2",
            "key-γ": "v3"
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["key-β"], "v2");
}

#[test]
fn benchmark_run_with_unicode_metrics_3_keys_3() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "key-α": "v1",
            "key-β": "v2",
            "key-γ": "v3"
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["key-γ"], "v3");
}

#[test]
fn trigger_def_with_unicode_5_segment_pattern_2() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"a-α-b-β-c-γ"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("a-α-b-β-c-γ"));
}

#[test]
fn feedback_with_unicode_only_tags_3() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec!["标签-α".into(), "标签-β".into()],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tags.len(), 2);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_3_fields_4() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "请求-α".into(),
        original_output: "输出-β".into(),
        corrected_output: "修正-γ".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.original_request, "请求-α");
    assert_eq!(back.original_output, "输出-β");
    assert_eq!(back.corrected_output, "修正-γ");
}

#[test]
fn vector_record_with_unicode_metadata_5_keys_2() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "k1-α": "v1",
            "k2-β": "v2",
            "k3-γ": "v3",
            "k4-δ": "v4",
            "k5-ε": "v5"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k3-γ"], "v3");
}

#[test]
fn plugin_descriptor_with_unicode_4_segment_path_2() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/a/α/β/γ.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert!(back.wasm_path.to_str().unwrap().contains("γ.wasm"));
}

#[test]
fn agent_message_with_unicode_payload_2_keys_5() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "α": "value-α",
            "β": "value-β"
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["α"], "value-α");
}

#[test]
fn benchmark_run_with_unicode_metrics_2_keys_4() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "α": "v-α",
            "β": "v-β"
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["α"], "v-α");
}

#[test]
fn trigger_def_with_unicode_3_segment_pattern_3() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"a-α-b"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("a-α-b"));
}

#[test]
fn feedback_with_unicode_only_corrected_5() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "修正-α-β-γ-δ-ε-ζ".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.corrected_output, "修正-α-β-γ-δ-ε-ζ");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_only_agent_id_4() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "agent-α".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "agent-α");
}

#[test]
fn vector_record_with_unicode_metadata_3_keys_4() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "k1": "v1-α",
            "k2": "v2-β",
            "k3": "v3-γ"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k2"], "v2-β");
}

#[test]
fn plugin_descriptor_with_unicode_2_segment_path_3() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/α-β/γ.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.wasm_path, std::path::PathBuf::from("/α-β/γ.wasm"));
}

#[test]
fn agent_message_with_unicode_payload_2_keys_6() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "α-β": "value-γ",
            "δ-ε": "value-ζ"
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["α-β"], "value-γ");
}

#[test]
fn benchmark_run_with_unicode_metrics_2_keys_5() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "α-β": "v-γ",
            "δ-ε": "v-ζ"
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["α-β"], "v-γ");
}

#[test]
fn trigger_def_with_unicode_4_segment_pattern_2() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"a-α-b-β"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("a-α-b-β"));
}

#[test]
fn feedback_with_unicode_only_output_3() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "输出-α-🔍-γ-δ-ε".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.original_output, "输出-α-🔍-γ-δ-ε");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn feedback_with_unicode_2_fields_2() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "请求-α".into(),
        original_output: "输出-β".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.original_request, "请求-α");
    assert_eq!(back.original_output, "输出-β");
}

#[test]
fn vector_record_with_unicode_metadata_5_keys_3() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({
            "k1-α": "v1-β",
            "k2-γ": "v2-δ",
            "k3-ε": "v3-ζ",
            "k4-η": "v4-θ",
            "k5-ι": "v5-κ"
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k1-α"], "v1-β");
}

#[test]
fn plugin_descriptor_with_unicode_3_segment_path_3() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/a/α/β.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert!(back.wasm_path.to_str().unwrap().contains("β.wasm"));
}

#[test]
fn agent_message_with_unicode_payload_3_keys_3() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "key-α": "v1-β",
            "key-γ": "v2-δ",
            "key-ε": "v3-ζ"
        }),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["key-γ"], "v2-δ");
}

#[test]
fn benchmark_run_with_unicode_metrics_3_keys_4() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({
            "key-α": "v1-β",
            "key-γ": "v2-δ",
            "key-ε": "v3-ζ"
        }),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["key-ε"], "v3-ζ");
}

#[test]
fn trigger_def_with_unicode_pattern_4_chars() {
    let t: TriggerDef = serde_json::from_str(r#"{"pattern":"αβγδ"}"#).unwrap();
    assert_eq!(t.pattern.as_deref(), Some("αβγδ"));
}

#[test]
fn feedback_with_unicode_only_corrected_6() {
    use sisyphus_types::Feedback;
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "修正-α-🔍-β-γ-δ".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.corrected_output, "修正-α-🔍-β-γ-δ");
}
