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
    let s3 = s.clone();
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
