//! Integration tests for sisyphus-intel simulation_gate.
//!
//! Exercises the public `SimulationGate` / `GateHistory` / `GateConfig` /
//! `GateDecision` / `GateVerdict` / `LogicalSimulator` API via a
//! `MockSimulator` impl. Verifies:
//! - disabled gate returns `Skipped`
//! - invalid SOP (is_valid=false) returns `Reject`
//! - SOP containing Critical flaws returns `Reject`
//! - valid SOP returns `Allow`
//! - simulator error + fallback_on_error=true returns `Skipped`
//! - simulator error + fallback_on_error=false returns Err
//! - GateHistory aggregates `intercept_rate` correctly
//! - GateVerdict::as_str mapping
//!
//! These tests are distinct from the in-crate `tests` mod so they exercise
//! the public surface from an external integration perspective.

use async_trait::async_trait;
use sisyphus_intel::simulation_gate::{
    FlawSeverity, GateConfig, GateDecision, GateHistory, GateVerdict, LogicalFlaw,
    LogicalSimulationResult, LogicalSimulator, SimulationGate,
};
use sisyphus_types::{ExecutionContext, IntelError, SOPGraph};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Mock simulator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum MockResponse {
    Ok(LogicalSimulationResult),
    Err,
}

struct MockSimulator {
    response: parking_lot::Mutex<Option<MockResponse>>,
}

impl MockSimulator {
    fn ok(result: LogicalSimulationResult) -> Arc<Self> {
        Arc::new(Self {
            response: parking_lot::Mutex::new(Some(MockResponse::Ok(result))),
        })
    }

    fn err() -> Arc<Self> {
        Arc::new(Self {
            response: parking_lot::Mutex::new(Some(MockResponse::Err)),
        })
    }
}

#[async_trait]
impl LogicalSimulator for MockSimulator {
    async fn simulate_logic(
        &self,
        _graph: Arc<SOPGraph>,
        _ctx: &ExecutionContext,
        _sop_id: &str,
        _goal: Option<&str>,
    ) -> Result<LogicalSimulationResult, IntelError> {
        match self.response.lock().take() {
            Some(MockResponse::Ok(r)) => Ok(r),
            Some(MockResponse::Err) => Err(IntelError::SimulationFailed { sop_id: "x".into(), reason: "forced".into() }),
            None => Ok(LogicalSimulationResult {
                is_valid: true,
                flaws: vec![],
                predicted_path: vec![],
                confidence: 1.0,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn empty_graph() -> Arc<SOPGraph> {
    Arc::new(SOPGraph::new())
}

fn empty_ctx() -> ExecutionContext {
    ExecutionContext::default()
}

fn valid_result() -> LogicalSimulationResult {
    LogicalSimulationResult {
        is_valid: true,
        flaws: vec![],
        predicted_path: vec![],
        confidence: 0.95,
    }
}

fn invalid_result() -> LogicalSimulationResult {
    LogicalSimulationResult {
        is_valid: false,
        flaws: vec![],
        predicted_path: vec![],
        confidence: 0.2,
    }
}

fn critical_result() -> LogicalSimulationResult {
    LogicalSimulationResult {
        is_valid: true, // even if is_valid=true, critical => Reject
        flaws: vec![LogicalFlaw {
            node_name: "n1".into(),
            description: "deadlock".into(),
            severity: FlawSeverity::Critical,
        }],
        predicted_path: vec!["n1".into()],
        confidence: 0.4,
    }
}

fn warning_only_result() -> LogicalSimulationResult {
    LogicalSimulationResult {
        is_valid: true,
        flaws: vec![LogicalFlaw {
            node_name: "n1".into(),
            description: "minor".into(),
            severity: FlawSeverity::Warning,
        }],
        predicted_path: vec!["n1".into()],
        confidence: 0.7,
    }
}

// ---------------------------------------------------------------------------
// GateVerdict
// ---------------------------------------------------------------------------

#[test]
fn gate_verdict_as_str() {
    assert_eq!(GateVerdict::Allow.as_str(), "allow");
    assert_eq!(GateVerdict::Reject.as_str(), "reject");
    assert_eq!(GateVerdict::Skipped.as_str(), "skipped");
}

// ---------------------------------------------------------------------------
// GateHistory aggregation
// ---------------------------------------------------------------------------

#[test]
fn gate_history_empty_intercept_rate_is_zero() {
    let h = GateHistory::new();
    assert_eq!(h.intercept_rate(), 0.0);
    assert_eq!(h.false_positive_rate(), 0.0);
    assert!(h.snapshot().is_empty());
}

#[test]
fn gate_history_intercept_rate_excludes_skipped() {
    let h = GateHistory::new();
    h.record(GateDecision {
        sop_id: "s1".into(),
        verdict: GateVerdict::Allow,
        flaw_count: 0,
        critical_count: 0,
        confidence: 0.9,
        elapsed_micros: 100,
    });
    h.record(GateDecision {
        sop_id: "s2".into(),
        verdict: GateVerdict::Reject,
        flaw_count: 1,
        critical_count: 1,
        confidence: 0.5,
        elapsed_micros: 200,
    });
    h.record(GateDecision {
        sop_id: "s3".into(),
        verdict: GateVerdict::Skipped,
        flaw_count: 0,
        critical_count: 0,
        confidence: 0.0,
        elapsed_micros: 0,
    });
    // 1 Reject / (1 Allow + 1 Reject) = 0.5
    assert!((h.intercept_rate() - 0.5).abs() < 1e-9);
    assert_eq!(h.snapshot().len(), 3);
}

#[test]
fn gate_history_intercept_rate_pure_allow() {
    let h = GateHistory::new();
    for i in 0..3 {
        h.record(GateDecision {
            sop_id: format!("s{i}"),
            verdict: GateVerdict::Allow,
            flaw_count: 0,
            critical_count: 0,
            confidence: 1.0,
            elapsed_micros: 50,
        });
    }
    assert_eq!(h.intercept_rate(), 0.0);
}

#[test]
fn gate_history_intercept_rate_pure_reject() {
    let h = GateHistory::new();
    for i in 0..3 {
        h.record(GateDecision {
            sop_id: format!("s{i}"),
            verdict: GateVerdict::Reject,
            flaw_count: 1,
            critical_count: 0,
            confidence: 0.0,
            elapsed_micros: 50,
        });
    }
    assert!((h.intercept_rate() - 1.0).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// GateConfig
// ---------------------------------------------------------------------------

#[test]
fn gate_config_default_is_enabled_with_fallback() {
    let c = GateConfig::default();
    assert!(c.enabled);
    assert!(c.fallback_on_error);
}

// ---------------------------------------------------------------------------
// SimulationGate::check
// ---------------------------------------------------------------------------

#[tokio::test]
async fn check_disabled_returns_skipped() {
    let sim = MockSimulator::ok(valid_result());
    let cfg = GateConfig {
        enabled: false,
        fallback_on_error: true,
    };
    let gate = SimulationGate::new(sim, cfg);
    let d = gate
        .check(empty_graph(), &empty_ctx(), "sop-1", Some("goal"))
        .await
        .expect("disabled check");
    assert_eq!(d.verdict, GateVerdict::Skipped);
    assert_eq!(d.flaw_count, 0);
    assert_eq!(d.critical_count, 0);
    assert_eq!(d.sop_id, "sop-1");
}

#[tokio::test]
async fn check_invalid_sop_returns_reject() {
    let sim = MockSimulator::ok(invalid_result());
    let gate = SimulationGate::new(sim, GateConfig::default());
    let d = gate
        .check(empty_graph(), &empty_ctx(), "bad", None)
        .await
        .expect("ok");
    assert_eq!(d.verdict, GateVerdict::Reject);
    assert_eq!(d.critical_count, 0);
}

#[tokio::test]
async fn check_critical_flaw_returns_reject() {
    let sim = MockSimulator::ok(critical_result());
    let gate = SimulationGate::new(sim, GateConfig::default());
    let d = gate
        .check(empty_graph(), &empty_ctx(), "crit", None)
        .await
        .expect("ok");
    assert_eq!(d.verdict, GateVerdict::Reject);
    assert_eq!(d.critical_count, 1);
    assert_eq!(d.flaw_count, 1);
}

#[tokio::test]
async fn check_warning_only_sop_returns_allow() {
    let sim = MockSimulator::ok(warning_only_result());
    let gate = SimulationGate::new(sim, GateConfig::default());
    let d = gate
        .check(empty_graph(), &empty_ctx(), "warn", None)
        .await
        .expect("ok");
    assert_eq!(d.verdict, GateVerdict::Allow);
    assert_eq!(d.flaw_count, 1);
    assert_eq!(d.critical_count, 0);
}

#[tokio::test]
async fn check_valid_sop_returns_allow() {
    let sim = MockSimulator::ok(valid_result());
    let gate = SimulationGate::new(sim, GateConfig::default());
    let d = gate
        .check(empty_graph(), &empty_ctx(), "ok", None)
        .await
        .expect("ok");
    assert_eq!(d.verdict, GateVerdict::Allow);
}

#[tokio::test]
async fn check_simulator_error_with_fallback_returns_skipped() {
    let sim = MockSimulator::err();
    let gate = SimulationGate::new(sim, GateConfig::default());
    let d = gate
        .check(empty_graph(), &empty_ctx(), "err", None)
        .await
        .expect("fallback ok");
    assert_eq!(d.verdict, GateVerdict::Skipped);
}

#[tokio::test]
async fn check_simulator_error_without_fallback_returns_err() {
    let sim = MockSimulator::err();
    let cfg = GateConfig {
        enabled: true,
        fallback_on_error: false,
    };
    let gate = SimulationGate::new(sim, cfg);
    let result = gate
        .check(empty_graph(), &empty_ctx(), "err", None)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn gate_records_decisions_in_shared_history() {
    let sim1 = MockSimulator::ok(valid_result());
    let sim2 = MockSimulator::ok(critical_result());
    let history = Arc::new(GateHistory::new());

    let g1 = SimulationGate::with_history(sim1, GateConfig::default(), history.clone());
    let g2 = SimulationGate::with_history(sim2, GateConfig::default(), history.clone());

    g1.check(empty_graph(), &empty_ctx(), "a", None)
        .await
        .expect("ok");
    g2.check(empty_graph(), &empty_ctx(), "b", None)
        .await
        .expect("ok");

    let snap = history.snapshot();
    assert_eq!(snap.len(), 2);
    // 1 Reject / (1 Allow + 1 Reject) = 0.5
    assert!((history.intercept_rate() - 0.5).abs() < 1e-9);

    // Accessors work
    assert_eq!(g1.history().snapshot().len(), 2);
    assert!(g1.config().enabled);
}

#[tokio::test]
async fn check_elapsed_micros_is_recorded() {
    let sim = MockSimulator::ok(valid_result());
    let gate = SimulationGate::new(sim, GateConfig::default());
    let d = gate
        .check(empty_graph(), &empty_ctx(), "t", None)
        .await
        .expect("ok");
    // elapsed_micros is u128, must be populated (>=0 is trivially true)
    // We just check that the field is set without overflowing or panicking.
    let _ = d.elapsed_micros;
}
