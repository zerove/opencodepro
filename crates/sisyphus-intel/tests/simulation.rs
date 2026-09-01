//! Integration tests for sisyphus-intel `simulation` types.
//!
//! Exercises the public `SimulationResult`, `LogicalSimulationResult`,
//! `LogicalFlaw`, and `FlawSeverity` types. These are the
//! structured outputs of the simulation phase: the Monte Carlo
//! simulator produces `SimulationResult`; the logical simulator
//! produces `LogicalSimulationResult` with a list of flaws.

use sisyphus_intel::simulation::{
    FlawSeverity, LogicalFlaw, LogicalSimulationResult, SimulationResult,
};

// ---------------------------------------------------------------------------
// FlawSeverity
// ---------------------------------------------------------------------------

#[test]
fn flaw_severity_equality() {
    assert_eq!(FlawSeverity::Critical, FlawSeverity::Critical);
    assert_ne!(FlawSeverity::Critical, FlawSeverity::Warning);
    assert_ne!(FlawSeverity::Warning, FlawSeverity::Info);
    assert_ne!(FlawSeverity::Critical, FlawSeverity::Info);
}

#[test]
fn flaw_severity_clone_copy() {
    let s = FlawSeverity::Critical;
    let s2 = s; // Copy
                // rustc 1.98 clippy::clone_on_copy 修复：直接 Copy 复制
    let s3 = s;
    let _ = s;
    assert_eq!(s2, s3);
}

// ---------------------------------------------------------------------------
// LogicalFlaw
// ---------------------------------------------------------------------------

#[test]
fn logical_flaw_serialization_roundtrip() {
    let flaw = LogicalFlaw {
        node_name: "node_a".into(),
        description: "missing input validation".into(),
        severity: FlawSeverity::Critical,
    };
    let json = serde_json::to_string(&flaw).expect("");
    let parsed: LogicalFlaw = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.node_name, "node_a");
    assert_eq!(parsed.description, "missing input validation");
    assert_eq!(parsed.severity, FlawSeverity::Critical);
}

#[test]
fn logical_flaw_clone() {
    let flaw = LogicalFlaw {
        node_name: "n".into(),
        description: "d".into(),
        severity: FlawSeverity::Warning,
    };
    let cloned = flaw.clone();
    assert_eq!(cloned.node_name, "n");
    assert_eq!(cloned.severity, FlawSeverity::Warning);
}

#[test]
fn logical_flaw_with_long_description() {
    let long = "x".repeat(1000);
    let flaw = LogicalFlaw {
        node_name: "n".into(),
        description: long.clone(),
        severity: FlawSeverity::Info,
    };
    assert_eq!(flaw.description.len(), 1000);
}

// ---------------------------------------------------------------------------
// LogicalSimulationResult
// ---------------------------------------------------------------------------

#[test]
fn logical_simulation_result_valid_no_flaws() {
    let r = LogicalSimulationResult {
        is_valid: true,
        flaws: vec![],
        predicted_path: vec!["a".into(), "b".into()],
        confidence: 0.95,
    };
    let json = serde_json::to_string(&r).expect("");
    let parsed: LogicalSimulationResult = serde_json::from_str(&json).expect("");
    assert!(parsed.is_valid);
    assert!(parsed.flaws.is_empty());
    assert_eq!(parsed.predicted_path.len(), 2);
    assert!((parsed.confidence - 0.95).abs() < 1e-9);
}

#[test]
fn logical_simulation_result_with_flaws() {
    let r = LogicalSimulationResult {
        is_valid: false,
        flaws: vec![
            LogicalFlaw {
                node_name: "a".into(),
                description: "d1".into(),
                severity: FlawSeverity::Critical,
            },
            LogicalFlaw {
                node_name: "b".into(),
                description: "d2".into(),
                severity: FlawSeverity::Warning,
            },
        ],
        predicted_path: vec!["a".into()],
        confidence: 0.3,
    };
    assert!(!r.is_valid);
    assert_eq!(r.flaws.len(), 2);
    assert!(r.flaws[0].severity == FlawSeverity::Critical);
    let json = serde_json::to_string(&r).expect("");
    let parsed: LogicalSimulationResult = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.flaws.len(), 2);
}

// ---------------------------------------------------------------------------
// SimulationResult
// ---------------------------------------------------------------------------

#[test]
fn simulation_result_basic() {
    let r = SimulationResult {
        path_id: "default".into(),
        success_probability: 0.85,
        estimated_cost: 100,
        predicted_stability: 0.9,
        predicted_effort: 5,
        predicted_outcome: "success".into(),
    };
    let json = serde_json::to_string(&r).expect("");
    let parsed: SimulationResult = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.path_id, "default");
    assert!((parsed.success_probability - 0.85).abs() < 1e-9);
    assert_eq!(parsed.estimated_cost, 100);
    assert_eq!(parsed.predicted_effort, 5);
    assert_eq!(parsed.predicted_outcome, "success");
}

#[test]
fn simulation_result_clone() {
    let r = SimulationResult {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        predicted_stability: 0.5,
        predicted_effort: 0,
        predicted_outcome: "unknown".into(),
    };
    let cloned = r.clone();
    assert_eq!(cloned.path_id, "p");
}

#[test]
fn simulation_result_with_zero_values() {
    let r = SimulationResult {
        path_id: "".into(),
        success_probability: 0.0,
        estimated_cost: 0,
        predicted_stability: 0.0,
        predicted_effort: 0,
        predicted_outcome: "".into(),
    };
    let json = serde_json::to_string(&r).expect("");
    let parsed: SimulationResult = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.success_probability, 0.0);
    assert_eq!(parsed.estimated_cost, 0);
}

#[test]
fn simulation_result_with_high_probability() {
    let r = SimulationResult {
        path_id: "certain".into(),
        success_probability: 1.0,
        estimated_cost: 1_000_000,
        predicted_stability: 1.0,
        predicted_effort: 100,
        predicted_outcome: "guaranteed".into(),
    };
    let json = serde_json::to_string(&r).expect("");
    let parsed: SimulationResult = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.success_probability, 1.0);
}

// ---------------------------------------------------------------------------
// Additional edge cases
// ---------------------------------------------------------------------------

#[test]
fn flaw_severity_debug_format() {
    // Debug formatting should not panic for any variant
    let _ = format!("{:?}", FlawSeverity::Critical);
    let _ = format!("{:?}", FlawSeverity::Warning);
    let _ = format!("{:?}", FlawSeverity::Info);
}

#[test]
fn flaw_severity_serde() {
    let json_c = serde_json::to_string(&FlawSeverity::Critical).expect("");
    let parsed: FlawSeverity = serde_json::from_str(&json_c).expect("");
    assert_eq!(parsed, FlawSeverity::Critical);

    let json_w = serde_json::to_string(&FlawSeverity::Warning).expect("");
    let parsed: FlawSeverity = serde_json::from_str(&json_w).expect("");
    assert_eq!(parsed, FlawSeverity::Warning);

    let json_i = serde_json::to_string(&FlawSeverity::Info).expect("");
    let parsed: FlawSeverity = serde_json::from_str(&json_i).expect("");
    assert_eq!(parsed, FlawSeverity::Info);
}

#[test]
fn logical_flaw_unicode_description() {
    let flaw = LogicalFlaw {
        node_name: "node_α".into(),
        description: "缺少输入校验 🚫".into(),
        severity: FlawSeverity::Critical,
    };
    let json = serde_json::to_string(&flaw).expect("");
    let parsed: LogicalFlaw = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.node_name, "node_α");
    assert_eq!(parsed.description, "缺少输入校验 🚫");
}

#[test]
fn logical_flaw_serde_variants() {
    // All three severities roundtrip via JSON
    for sev in [
        FlawSeverity::Critical,
        FlawSeverity::Warning,
        FlawSeverity::Info,
    ] {
        let flaw = LogicalFlaw {
            node_name: "n".into(),
            description: "d".into(),
            severity: sev, // Copy 即可
        };
        let json = serde_json::to_string(&flaw).expect("");
        let parsed: LogicalFlaw = serde_json::from_str(&json).expect("");
        assert_eq!(parsed.severity, sev);
    }
}

#[test]
fn logical_simulation_result_zero_confidence() {
    let r = LogicalSimulationResult {
        is_valid: false,
        flaws: vec![LogicalFlaw {
            node_name: "n".into(),
            description: "d".into(),
            severity: FlawSeverity::Critical,
        }],
        predicted_path: vec![],
        confidence: 0.0,
    };
    let json = serde_json::to_string(&r).expect("");
    let parsed: LogicalSimulationResult = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.confidence, 0.0);
    assert!(parsed.predicted_path.is_empty());
}

#[test]
fn logical_simulation_result_clone() {
    let r = LogicalSimulationResult {
        is_valid: true,
        flaws: vec![],
        predicted_path: vec!["x".into()],
        confidence: 0.5,
    };
    let cloned = r.clone();
    assert_eq!(cloned.is_valid, r.is_valid);
    assert_eq!(cloned.confidence, r.confidence);
    assert_eq!(cloned.predicted_path, r.predicted_path);
}

#[test]
fn logical_simulation_result_debug() {
    let r = LogicalSimulationResult {
        is_valid: true,
        flaws: vec![],
        predicted_path: vec!["a".into()],
        confidence: 0.9,
    };
    let dbg = format!("{:?}", r);
    assert!(dbg.contains("is_valid"));
    assert!(dbg.contains("confidence"));
}

#[test]
fn simulation_result_unicode_outcome() {
    let r = SimulationResult {
        path_id: "路径-1".into(),
        success_probability: 0.5,
        estimated_cost: 42,
        predicted_stability: 0.5,
        predicted_effort: 1,
        predicted_outcome: "成功 🎉".into(),
    };
    let json = serde_json::to_string(&r).expect("");
    let parsed: SimulationResult = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.path_id, "路径-1");
    assert_eq!(parsed.predicted_outcome, "成功 🎉");
}

#[test]
fn simulation_result_debug_format() {
    let r = SimulationResult {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        predicted_stability: 0.0,
        predicted_effort: 0,
        predicted_outcome: "x".into(),
    };
    let dbg = format!("{:?}", r);
    assert!(dbg.contains("path_id"));
    assert!(dbg.contains("predicted_outcome"));
}

#[test]
fn simulation_result_max_u64_cost() {
    let r = SimulationResult {
        path_id: "max".into(),
        success_probability: 0.0,
        estimated_cost: u64::MAX,
        predicted_stability: 0.0,
        predicted_effort: u64::MAX,
        predicted_outcome: "overflow".into(),
    };
    let json = serde_json::to_string(&r).expect("");
    let parsed: SimulationResult = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.estimated_cost, u64::MAX);
    assert_eq!(parsed.predicted_effort, u64::MAX);
}

// ---------------------------------------------------------------------------
// Additional edge cases
// ---------------------------------------------------------------------------

#[test]
fn logical_simulation_result_with_many_flaws() {
    let flaws: Vec<LogicalFlaw> = (0..100)
        .map(|i| LogicalFlaw {
            node_name: format!("n{}", i),
            description: format!("d{}", i),
            severity: FlawSeverity::Warning,
        })
        .collect();
    let r = LogicalSimulationResult {
        is_valid: false,
        flaws,
        predicted_path: vec![],
        confidence: 0.0,
    };
    assert_eq!(r.flaws.len(), 100);
    let j = serde_json::to_string(&r).unwrap();
    let back: LogicalSimulationResult = serde_json::from_str(&j).unwrap();
    assert_eq!(back.flaws.len(), 100);
    assert_eq!(back.flaws[42].node_name, "n42");
}

#[test]
fn logical_simulation_result_with_long_predicted_path() {
    let path: Vec<String> = (0..50).map(|i| format!("step-{}", i)).collect();
    let r = LogicalSimulationResult {
        is_valid: true,
        flaws: vec![],
        predicted_path: path.clone(),
        confidence: 0.99,
    };
    assert_eq!(r.predicted_path.len(), 50);
    let j = serde_json::to_string(&r).unwrap();
    let back: LogicalSimulationResult = serde_json::from_str(&j).unwrap();
    assert_eq!(back.predicted_path[49], "step-49");
}

#[test]
fn flaw_severity_inequality() {
    assert_ne!(FlawSeverity::Critical, FlawSeverity::Warning);
    assert_ne!(FlawSeverity::Critical, FlawSeverity::Info);
    assert_ne!(FlawSeverity::Warning, FlawSeverity::Info);
}

#[test]
fn logical_flaw_with_empty_strings() {
    let flaw = LogicalFlaw {
        node_name: "".into(),
        description: "".into(),
        severity: FlawSeverity::Info,
    };
    assert_eq!(flaw.node_name, "");
    let j = serde_json::to_string(&flaw).unwrap();
    let back: LogicalFlaw = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, "");
}

#[test]
fn logical_flaw_unicode_node_name() {
    let flaw = LogicalFlaw {
        node_name: "节点-α".into(),
        description: "中文问题".into(),
        severity: FlawSeverity::Critical,
    };
    let j = serde_json::to_string(&flaw).unwrap();
    let back: LogicalFlaw = serde_json::from_str(&j).unwrap();
    assert_eq!(back.node_name, "节点-α");
}

#[test]
fn simulation_result_unicode_path_id() {
    let r = SimulationResult {
        path_id: "路径-α".into(),
        success_probability: 0.5,
        estimated_cost: 100,
        predicted_stability: 0.5,
        predicted_effort: 5,
        predicted_outcome: "中文结果".into(),
    };
    assert_eq!(r.path_id, "路径-α");
    let j = serde_json::to_string(&r).unwrap();
    let back: SimulationResult = serde_json::from_str(&j).unwrap();
    assert_eq!(back.predicted_outcome, "中文结果");
}
