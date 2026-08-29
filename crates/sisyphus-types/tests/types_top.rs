//! Integration tests for sisyphus-types `types` module (top-level).
//!
//! Exercises the public plain-data types in `sisyphus_types::types`:
//! `Feedback`, `PluginDescriptor`, `VectorRecord`, `AgentMessage`,
//! `BenchmarkRun`, `PlatformConfig`, `FormalVerificationConfig`,
//! `MetaLearningSample`, `CustomToolDefinition`, `TriggerDef`.
//! All are pure data with serde derives, so tests focus on
//! serialization round-trips, defaults, and clone semantics.

use sisyphus_types::{
    AgentMessage, BenchmarkRun, CustomToolDefinition, Feedback, FormalVerificationConfig,
    MetaLearningSample, PlatformConfig, PluginDescriptor, TriggerDef, VectorRecord,
};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Feedback
// ---------------------------------------------------------------------------

#[test]
fn feedback_serialization_roundtrip() {
    let f = Feedback {
        agent_id: "agent-1".into(),
        original_request: "do X".into(),
        original_output: "X done badly".into(),
        corrected_output: "X done well".into(),
        timestamp: 1_700_000_000,
        tags: vec!["bug".into(), "logic".into()],
    };
    let json = serde_json::to_string(&f).expect("");
    let parsed: Feedback = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.agent_id, "agent-1");
    assert_eq!(parsed.original_request, "do X");
    assert_eq!(parsed.corrected_output, "X done well");
    assert_eq!(parsed.tags.len(), 2);
}

#[test]
fn feedback_with_empty_tags() {
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "x".into(),
        original_output: "y".into(),
        corrected_output: "z".into(),
        timestamp: 0,
        tags: vec![],
    };
    let json = serde_json::to_string(&f).expect("");
    let parsed: Feedback = serde_json::from_str(&json).expect("");
    assert!(parsed.tags.is_empty());
}

#[test]
fn feedback_clone() {
    let f = Feedback {
        agent_id: "a".into(),
        original_request: "r".into(),
        original_output: "o".into(),
        corrected_output: "c".into(),
        timestamp: 100,
        tags: vec!["t".into()],
    };
    let cloned = f.clone();
    assert_eq!(cloned.agent_id, "a");
    assert_eq!(cloned.tags.len(), 1);
}

// ---------------------------------------------------------------------------
// PluginDescriptor
// ---------------------------------------------------------------------------

#[test]
fn plugin_descriptor_serialization() {
    let p = PluginDescriptor {
        name: "my_plugin".into(),
        version: "1.0.0".into(),
        wasm_path: PathBuf::from("/tmp/plugin.wasm"),
        tool_id: "my_tool".into(),
        fuel_limit: 1000,
        checksum: Some("abc123".into()),
    };
    let json = serde_json::to_string(&p).expect("");
    let parsed: PluginDescriptor = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.name, "my_plugin");
    assert_eq!(parsed.version, "1.0.0");
    assert_eq!(parsed.wasm_path, PathBuf::from("/tmp/plugin.wasm"));
    assert_eq!(parsed.fuel_limit, 1000);
    assert_eq!(parsed.checksum.as_deref(), Some("abc123"));
}

#[test]
fn plugin_descriptor_without_checksum_uses_default() {
    // No `checksum` field in JSON → should default to None.
    let json = r#"{
        "name": "p",
        "version": "1.0",
        "wasm_path": "/p.wasm",
        "tool_id": "t",
        "fuel_limit": 100
    }"#;
    let p: PluginDescriptor = serde_json::from_str(json).expect("");
    assert_eq!(p.name, "p");
    assert!(p.checksum.is_none());
}

// ---------------------------------------------------------------------------
// VectorRecord
// ---------------------------------------------------------------------------

#[test]
fn vector_record_serialization() {
    let v = VectorRecord {
        key: "vec-1".into(),
        embedding: vec![0.1, 0.2, 0.3, 0.4],
        metadata: serde_json::json!({"source": "test"}),
        timestamp: 1_700_000_000,
    };
    let json = serde_json::to_string(&v).expect("");
    let parsed: VectorRecord = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.key, "vec-1");
    assert_eq!(parsed.embedding.len(), 4);
    assert_eq!(parsed.metadata["source"], "test");
}

#[test]
fn vector_record_clone() {
    let v = VectorRecord {
        key: "x".into(),
        embedding: vec![0.0],
        metadata: serde_json::json!(null),
        timestamp: 0,
    };
    let cloned = v.clone();
    assert_eq!(cloned.key, "x");
    assert_eq!(cloned.embedding.len(), 1);
}

// ---------------------------------------------------------------------------
// AgentMessage
// ---------------------------------------------------------------------------

#[test]
fn agent_message_request_kind() {
    let m = AgentMessage {
        from: "alice".into(),
        to: "bob".into(),
        kind: "request".into(),
        payload: serde_json::json!({"q": "hello"}),
        correlation_id: "corr-1".into(),
    };
    let json = serde_json::to_string(&m).expect("");
    let parsed: AgentMessage = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.from, "alice");
    assert_eq!(parsed.kind, "request");
    assert_eq!(parsed.correlation_id, "corr-1");
}

#[test]
fn agent_message_response_kind() {
    let m = AgentMessage {
        from: "bob".into(),
        to: "alice".into(),
        kind: "response".into(),
        payload: serde_json::json!({"a": 42}),
        correlation_id: "corr-1".into(),
    };
    let json = serde_json::to_string(&m).expect("");
    let parsed: AgentMessage = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.kind, "response");
    assert_eq!(parsed.payload["a"], 42);
}

#[test]
fn agent_message_publish_subscribe_kinds() {
    for kind in ["publish", "subscribe"] {
        let m = AgentMessage {
            from: "x".into(),
            to: "y".into(),
            kind: kind.into(),
            payload: serde_json::json!({}),
            correlation_id: "c".into(),
        };
        let json = serde_json::to_string(&m).expect("");
        let parsed: AgentMessage = serde_json::from_str(&json).expect("");
        assert_eq!(parsed.kind, kind);
    }
}

// ---------------------------------------------------------------------------
// BenchmarkRun
// ---------------------------------------------------------------------------

#[test]
fn benchmark_run_serialization() {
    let b = BenchmarkRun {
        name: "bench1".into(),
        timestamp: 1_700_000_000,
        duration_secs: 5.5,
        success: true,
        metrics: serde_json::json!({"iterations": 100, "p99_ms": 12.3}),
    };
    let json = serde_json::to_string(&b).expect("");
    let parsed: BenchmarkRun = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.name, "bench1");
    assert!(parsed.success);
    assert!((parsed.duration_secs - 5.5).abs() < 1e-9);
    assert_eq!(parsed.metrics["iterations"], 100);
}

// ---------------------------------------------------------------------------
// PlatformConfig / FormalVerificationConfig / MetaLearningSample / CustomToolDefinition / TriggerDef
// (just ensure they can be round-tripped — they're pure data)
// ---------------------------------------------------------------------------

#[test]
fn platform_config_serialization() {
    let c = PlatformConfig {
        is_windows: false,
        path_sep: "/".into(),
        shell: "/bin/bash".into(),
    };
    let json = serde_json::to_string(&c).expect("");
    let parsed: PlatformConfig = serde_json::from_str(&json).expect("");
    assert!(!parsed.is_windows);
    assert_eq!(parsed.shell, "/bin/bash");
}

#[test]
fn formal_verification_config_serialization() {
    let c = FormalVerificationConfig {
        enable_kani: true,
        enable_loom: true,
        enable_proptest: true,
        max_proptest_cases: 1000,
    };
    let json = serde_json::to_string(&c).expect("");
    let parsed: FormalVerificationConfig = serde_json::from_str(&json).expect("");
    assert!(parsed.enable_kani);
    assert_eq!(parsed.max_proptest_cases, 1000);
}

#[test]
fn meta_learning_sample_serialization() {
    let m = MetaLearningSample {
        context: serde_json::json!([1.0, 2.0, 3.0]),
        strategy: "balanced".into(),
        params: serde_json::json!({}),
        success: true,
        duration_secs: 1.5,
        timestamp: 0,
    };
    let json = serde_json::to_string(&m).expect("");
    let parsed: MetaLearningSample = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.strategy, "balanced");
    assert!(parsed.success);
}

#[test]
fn custom_tool_definition_serialization() {
    let t = CustomToolDefinition {
        name: "my_tool".into(),
        description: "does X".into(),
        handler: PathBuf::from("/tmp/my_tool.wasm"),
        schema: serde_json::json!({"type": "object"}),
        fuel_limit: 1000,
    };
    let json = serde_json::to_string(&t).expect("");
    let parsed: CustomToolDefinition = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.name, "my_tool");
    assert_eq!(parsed.handler, PathBuf::from("/tmp/my_tool.wasm"));
}

#[test]
fn trigger_def_with_pattern() {
    let t = TriggerDef {
        pattern: Some("deploy".into()),
        tools: vec!["deploy_tool".into()],
        description: Some("deploy trigger".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("log".into()),
        hook_message: None,
    };
    let json = serde_json::to_string(&t).expect("");
    let parsed: TriggerDef = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.pattern.as_deref(), Some("deploy"));
    assert_eq!(parsed.tools.len(), 1);
}

#[test]
fn trigger_def_without_pattern() {
    let t = TriggerDef {
        pattern: None,
        tools: vec!["a".into(), "b".into()],
        description: None,
        hook_phase: None,
        hook_effect: None,
        hook_message: None,
    };
    let json = serde_json::to_string(&t).expect("");
    let parsed: TriggerDef = serde_json::from_str(&json).expect("");
    assert!(parsed.pattern.is_none());
    assert_eq!(parsed.tools.len(), 2);
}
