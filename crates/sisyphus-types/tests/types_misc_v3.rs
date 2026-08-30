//! Integration tests for sisyphus-types `types` module's
//! additional structs: `Feedback`, `PluginDescriptor`, `VectorRecord`,
//! `AgentMessage`, `BenchmarkRun`, `PlatformConfig`, and
//! `CognitiveEvent` enum.

use sisyphus_types::types::{
    AgentMessage, BenchmarkRun, CognitiveEvent, Feedback, PlatformConfig, PluginDescriptor,
    VectorRecord,
};

// ---------------------------------------------------------------------------
// Feedback
// ---------------------------------------------------------------------------

#[test]
fn feedback_field_assignment() {
    let f = Feedback {
        agent_id: "agent_1".into(),
        original_request: "do X".into(),
        original_output: "did Y".into(),
        corrected_output: "should have done Z".into(),
        timestamp: 1700000000,
        tags: vec!["accuracy".into(), "high-priority".into()],
    };
    assert_eq!(f.agent_id, "agent_1");
    assert_eq!(f.timestamp, 1700000000);
    assert_eq!(f.tags.len(), 2);
}

#[test]
fn feedback_serde_round_trip() {
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 42,
        tags: vec!["t1".into()],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "a");
    assert_eq!(back.timestamp, 42);
    assert_eq!(back.tags, vec!["t1"]);
}

#[test]
fn feedback_clone() {
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 0,
        tags: vec![],
    };
    let cloned = f.clone();
    assert_eq!(cloned.agent_id, "a");
    assert!(cloned.tags.is_empty());
}

#[test]
fn feedback_empty_tags() {
    let f = Feedback {
        agent_id: "".into(),
        original_request: "".into(),
        original_output: "".into(),
        corrected_output: "".into(),
        timestamp: 0,
        tags: vec![],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: Feedback = serde_json::from_str(&j).unwrap();
    assert_eq!(back.agent_id, "");
}

// ---------------------------------------------------------------------------
// PluginDescriptor
// ---------------------------------------------------------------------------

#[test]
fn plugin_descriptor_field_assignment() {
    let pd = PluginDescriptor {
        name: "test_plugin".into(),
        version: "1.0.0".into(),
        wasm_path: std::path::PathBuf::from("/tmp/plugin.wasm"),
        tool_id: "tool_x".into(),
        fuel_limit: 1_000_000,
        checksum: Some("abc123".into()),
    };
    assert_eq!(pd.name, "test_plugin");
    assert_eq!(pd.fuel_limit, 1_000_000);
    assert_eq!(pd.checksum.as_deref(), Some("abc123"));
}

#[test]
fn plugin_descriptor_serde() {
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "0.1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 100,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tool_id, "t");
    assert_eq!(back.checksum, None);
}

#[test]
fn plugin_descriptor_clone() {
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/x.wasm"),
        tool_id: "t".into(),
        fuel_limit: 0,
        checksum: None,
    };
    let cloned = pd.clone();
    assert_eq!(cloned.name, "p");
    assert_eq!(cloned.fuel_limit, 0);
}

// ---------------------------------------------------------------------------
// VectorRecord
// ---------------------------------------------------------------------------

#[test]
fn vector_record_field_assignment() {
    let vr = VectorRecord {
        key: "k1".into(),
        embedding: vec![0.1, 0.2, 0.3, 0.4],
        metadata: serde_json::json!({"source": "test"}),
        timestamp: 12345,
    };
    assert_eq!(vr.key, "k1");
    assert_eq!(vr.embedding.len(), 4);
    assert_eq!(vr.timestamp, 12345);
}

#[test]
fn vector_record_serde() {
    let vr = VectorRecord {
        key: "k".into(),
        embedding: vec![1.0, 2.0],
        metadata: serde_json::json!({}),
        timestamp: 99,
    };
    let j = serde_json::to_string(&vr).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.key, "k");
    assert_eq!(back.embedding, vec![1.0, 2.0]);
}

#[test]
fn vector_record_empty_embedding() {
    let vr = VectorRecord {
        key: "empty".into(),
        embedding: vec![],
        metadata: serde_json::json!(null),
        timestamp: 0,
    };
    assert!(vr.embedding.is_empty());
    let j = serde_json::to_string(&vr).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert!(back.embedding.is_empty());
}

#[test]
fn vector_record_clone() {
    let vr = VectorRecord {
        key: "k".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({}),
        timestamp: 1,
    };
    let cloned = vr.clone();
    assert_eq!(cloned.embedding, vec![0.5]);
}

// ---------------------------------------------------------------------------
// CognitiveEvent
// ---------------------------------------------------------------------------

#[test]
fn cognitive_event_predict_variant() {
    let ev = CognitiveEvent::Predict {
        prediction: serde_json::json!({"next": "A"}),
    };
    let j = serde_json::to_string(&ev).unwrap();
    let back: CognitiveEvent = serde_json::from_str(&j).unwrap();
    match back {
        CognitiveEvent::Predict { prediction } => {
            assert_eq!(prediction["next"], "A");
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn cognitive_event_reflect_variant() {
    let ev = CognitiveEvent::Reflect {
        report: serde_json::json!({"score": 0.5}),
    };
    let j = serde_json::to_string(&ev).unwrap();
    let back: CognitiveEvent = serde_json::from_str(&j).unwrap();
    match back {
        CognitiveEvent::Reflect { report } => {
            assert_eq!(report["score"], 0.5);
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn cognitive_event_evolve_variant() {
    let ev = CognitiveEvent::Evolve {
        revised_plan: sisyphus_types::graph::SOPGraphConfig::default(),
    };
    let j = serde_json::to_string(&ev).unwrap();
    let back: CognitiveEvent = serde_json::from_str(&j).unwrap();
    match back {
        CognitiveEvent::Evolve { .. } => {}
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn cognitive_event_clone() {
    let ev = CognitiveEvent::Predict {
        prediction: serde_json::json!({"a": 1}),
    };
    let cloned = ev.clone();
    match cloned {
        CognitiveEvent::Predict { prediction } => {
            assert_eq!(prediction["a"], 1);
        }
        _ => panic!("Wrong variant"),
    }
}

// ---------------------------------------------------------------------------
// AgentMessage
// ---------------------------------------------------------------------------

#[test]
fn agent_message_field_assignment() {
    let m = AgentMessage {
        from: "alice".into(),
        to: "bob".into(),
        kind: "request".into(),
        payload: serde_json::json!({"cmd": "run"}),
        correlation_id: "corr-1".into(),
    };
    assert_eq!(m.from, "alice");
    assert_eq!(m.to, "bob");
    assert_eq!(m.kind, "request");
    assert_eq!(m.correlation_id, "corr-1");
}

#[test]
fn agent_message_serde() {
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "response".into(),
        payload: serde_json::json!({"result": "ok"}),
        correlation_id: "corr-2".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from, "a");
    assert_eq!(back.kind, "response");
    assert_eq!(back.correlation_id, "corr-2");
}

#[test]
fn agent_message_clone() {
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "publish".into(),
        payload: serde_json::json!({}),
        correlation_id: "x".into(),
    };
    let cloned = m.clone();
    assert_eq!(cloned.kind, "publish");
}

// ---------------------------------------------------------------------------
// BenchmarkRun
// ---------------------------------------------------------------------------

#[test]
fn benchmark_run_field_assignment() {
    let b = BenchmarkRun {
        name: "cognition_loop".into(),
        timestamp: 1700000000,
        duration_secs: 12.5,
        success: true,
        metrics: serde_json::json!({"throughput": 1000}),
    };
    assert_eq!(b.name, "cognition_loop");
    assert!(b.success);
    assert_eq!(b.duration_secs, 12.5);
}

#[test]
fn benchmark_run_serde() {
    let b = BenchmarkRun {
        name: "test".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: false,
        metrics: serde_json::json!({}),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "test");
    assert!(!back.success);
}

#[test]
fn benchmark_run_clone() {
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 1,
        duration_secs: 0.5,
        success: true,
        metrics: serde_json::json!({"k": "v"}),
    };
    let cloned = b.clone();
    assert_eq!(cloned.name, "n");
    assert_eq!(cloned.metrics["k"], "v");
}

// ---------------------------------------------------------------------------
// PlatformConfig
// ---------------------------------------------------------------------------

#[test]
fn platform_config_default() {
    let p = PlatformConfig::default();
    // Default values: is_windows false, others empty
    assert!(!p.is_windows);
    assert_eq!(p.path_sep, "");
    assert_eq!(p.shell, "");
}

#[test]
fn platform_config_field_assignment() {
    let p = PlatformConfig {
        is_windows: true,
        path_sep: "\\".into(),
        shell: "powershell.exe".into(),
    };
    assert!(p.is_windows);
    assert_eq!(p.path_sep, "\\");
    assert_eq!(p.shell, "powershell.exe");
}

#[test]
fn platform_config_serde() {
    let p = PlatformConfig {
        is_windows: false,
        path_sep: "/".into(),
        shell: "/bin/bash".into(),
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: PlatformConfig = serde_json::from_str(&j).unwrap();
    assert!(!back.is_windows);
    assert_eq!(back.shell, "/bin/bash");
}

#[test]
fn platform_config_clone() {
    let p = PlatformConfig::default();
    let cloned = p.clone();
    assert_eq!(cloned.path_sep, "");
}

// ---------------------------------------------------------------------------
// Additional PluginDescriptor and VectorRecord coverage
// ---------------------------------------------------------------------------

#[test]
fn plugin_descriptor_with_max_fuel_limit() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/x.wasm"),
        tool_id: "t".into(),
        fuel_limit: u64::MAX,
        checksum: None,
    };
    assert_eq!(pd.fuel_limit, u64::MAX);
}

#[test]
fn plugin_descriptor_zero_fuel_limit() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/x.wasm"),
        tool_id: "t".into(),
        fuel_limit: 0,
        checksum: None,
    };
    assert_eq!(pd.fuel_limit, 0);
}

#[test]
fn vector_record_with_max_timestamp() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![],
        metadata: serde_json::json!({}),
        timestamp: u64::MAX,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.timestamp, u64::MAX);
}

#[test]
fn vector_record_clone_with_metadata() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![1.0, 2.0],
        metadata: serde_json::json!({"a": 1}),
        timestamp: 0,
    };
    let cloned = v.clone();
    assert_eq!(cloned.key, "k");
    assert_eq!(cloned.embedding, vec![1.0, 2.0]);
}

#[test]
fn agent_message_clone_preserves_payload() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({"key": "value"}),
        correlation_id: "c".into(),
    };
    let cloned = m.clone();
    assert_eq!(cloned.payload["key"], "value");
}

#[test]
fn benchmark_run_debug_format() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.0,
        success: false,
        metrics: serde_json::json!({}),
    };
    let dbg = format!("{:?}", b);
    assert!(dbg.contains("BenchmarkRun"));
}

#[test]
fn platform_config_debug_format() {
    use sisyphus_types::PlatformConfig;
    let p = PlatformConfig {
        is_windows: false,
        path_sep: "/".into(),
        shell: "sh".into(),
    };
    let dbg = format!("{:?}", p);
    assert!(dbg.contains("PlatformConfig"));
}

// ---------------------------------------------------------------------------
// Additional PluginDescriptor tests
// ---------------------------------------------------------------------------

#[test]
fn plugin_descriptor_with_unicode_path() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/路径/插件.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.wasm_path.to_str().unwrap(), "/路径/插件.wasm");
}

#[test]
fn plugin_descriptor_with_unicode_tool_id() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "工具-α".into(),
        fuel_limit: 1000,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.tool_id, "工具-α");
}

#[test]
fn plugin_descriptor_clone_with_checksum() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "p".into(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 1000,
        checksum: Some("abc".into()),
    };
    let cloned = pd.clone();
    assert_eq!(cloned.name, "p");
    assert_eq!(cloned.checksum, Some("abc".into()));
}

#[test]
fn vector_record_with_unicode_key() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "键-α".into(),
        embedding: vec![0.5],
        metadata: serde_json::json!({}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.key, "键-α");
}

#[test]
fn vector_record_with_unicode_metadata() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![],
        metadata: serde_json::json!({"作者": "张-α", "标签": "中文"}),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["作者"], "张-α");
    assert_eq!(back.metadata["标签"], "中文");
}

#[test]
fn agent_message_debug_format() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({}),
        correlation_id: "c".into(),
    };
    let dbg = format!("{:?}", m);
    assert!(dbg.contains("AgentMessage"));
}

#[test]
fn agent_message_unicode_kind() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "a".into(),
        to: "b".into(),
        kind: "发布-请求".into(),
        payload: serde_json::json!({}),
        correlation_id: "c".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.kind, "发布-请求");
}

// ---------------------------------------------------------------------------
// Additional PluginDescriptor and BenchmarkRun tests
// ---------------------------------------------------------------------------

#[test]
fn benchmark_run_with_negative_duration() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: -1.0,
        success: false,
        metrics: serde_json::json!({}),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.duration_secs, -1.0);
}

#[test]
fn benchmark_run_with_max_timestamp() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: u64::MAX,
        duration_secs: 0.0,
        success: true,
        metrics: serde_json::json!({}),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.timestamp, u64::MAX);
}

#[test]
fn benchmark_run_with_nan_duration() {
    use sisyphus_types::BenchmarkRun;
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: f64::NAN,
        success: true,
        metrics: serde_json::json!({}),
    };
    // NaN might not roundtrip via Serde, but should not panic
    let j = serde_json::to_string(&b).unwrap_or_default();
    let _ = j;
}

#[test]
fn plugin_descriptor_with_empty_name() {
    use sisyphus_types::PluginDescriptor;
    let pd = PluginDescriptor {
        name: "".into(),
        version: "".into(),
        wasm_path: std::path::PathBuf::from(""),
        tool_id: "".into(),
        fuel_limit: 0,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "");
}

#[test]
fn platform_config_with_unicode_shell() {
    use sisyphus_types::PlatformConfig;
    let p = PlatformConfig {
        is_windows: false,
        path_sep: "/".into(),
        shell: "/bin/中文-shell".into(),
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: PlatformConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.shell, "/bin/中文-shell");
}

#[test]
fn vector_record_with_unicode_metadata_complex() {
    use sisyphus_types::VectorRecord;
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![0.1, 0.2],
        metadata: serde_json::json!({
            "作者": "张-α",
            "嵌套": {"深-β": [1, 2, 3]},
            "布尔": true,
            "空值": null
        }),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["作者"], "张-α");
    assert_eq!(back.metadata["嵌套"]["深-β"][0], 1);
    assert_eq!(back.metadata["布尔"], true);
}

#[test]
fn agent_message_with_complex_payload_round_trip() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "α".into(),
        to: "β".into(),
        kind: "request".into(),
        payload: serde_json::json!({
            "data": {
                "list": [1, 2, 3],
                "obj": {"key": "val"},
                "num": 42.5,
                "str": "hello"
            }
        }),
        correlation_id: "γ".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: AgentMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.payload["data"]["list"][1], 2);
    assert_eq!(back.payload["data"]["obj"]["key"], "val");
    assert_eq!(back.payload["data"]["num"], 42.5);
}

// ---------------------------------------------------------------------------
// Additional PluginDescriptor and VectorRecord tests
// ---------------------------------------------------------------------------

#[test]
fn plugin_descriptor_with_max_version_string() {
    use sisyphus_types::PluginDescriptor;
    let long_version = "v".repeat(1000);
    let pd = PluginDescriptor {
        name: "p".into(),
        version: long_version.clone(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 100,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.version.len(), 1000);
}

#[test]
fn plugin_descriptor_with_max_name_string() {
    use sisyphus_types::PluginDescriptor;
    let long_name = "n".repeat(1000);
    let pd = PluginDescriptor {
        name: long_name.clone(),
        version: "1".into(),
        wasm_path: std::path::PathBuf::from("/p.wasm"),
        tool_id: "t".into(),
        fuel_limit: 100,
        checksum: None,
    };
    let j = serde_json::to_string(&pd).unwrap();
    let back: PluginDescriptor = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name.len(), 1000);
}

#[test]
fn vector_record_with_unicode_key_in_hash() {
    use sisyphus_types::VectorRecord;
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert("键-α".to_string(), vec![0.5_f32; 100]);
    let v = VectorRecord {
        key: "k".into(),
        embedding: vec![],
        metadata: serde_json::to_value(&map).unwrap(),
        timestamp: 0,
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: VectorRecord = serde_json::from_str(&j).unwrap();
    assert!(back.metadata["键-α"].is_array());
}

#[test]
fn agent_message_with_only_unicode_from() {
    use sisyphus_types::AgentMessage;
    let m = AgentMessage {
        from: "发信人-α".into(),
        to: "b".into(),
        kind: "request".into(),
        payload: serde_json::json!({}),
        correlation_id: "c".into(),
    };
    assert_eq!(m.from, "发信人-α");
}

#[test]
fn benchmark_run_with_max_metrics_complex() {
    use sisyphus_types::BenchmarkRun;
    let metrics = serde_json::json!({
        "name": "test",
        "iterations": 1000,
        "mean_ms": 0.5,
        "stddev": 0.1,
        "tags": ["fast", "reliable"]
    });
    let b = BenchmarkRun {
        name: "n".into(),
        timestamp: 0,
        duration_secs: 0.5,
        success: true,
        metrics,
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BenchmarkRun = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metrics["iterations"], 1000);
    assert_eq!(back.metrics["tags"][1], "reliable");
}

#[test]
fn platform_config_with_all_long_strings() {
    use sisyphus_types::PlatformConfig;
    let long = "x".repeat(1000);
    let p = PlatformConfig {
        is_windows: false,
        path_sep: long.clone(),
        shell: long.clone(),
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: PlatformConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.path_sep.len(), 1000);
    assert_eq!(back.shell.len(), 1000);
}
