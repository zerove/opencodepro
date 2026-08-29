//! Integration tests for sisyphus-intel `simulation_gate` module.
//!
//! Covers the pure-data and counter-only parts of the gate API:
//! [`GateDecision`], [`GateVerdict`], [`GateHistory`] (record/snapshot/
//! intercept_rate), and [`GateConfig`] default values.

use sisyphus_intel::simulation_gate::{GateConfig, GateDecision, GateHistory, GateVerdict};

// ---------------------------------------------------------------------------
// GateVerdict
// ---------------------------------------------------------------------------

#[test]
fn gate_verdict_as_str_allow() {
    assert_eq!(GateVerdict::Allow.as_str(), "allow");
}

#[test]
fn gate_verdict_as_str_reject() {
    assert_eq!(GateVerdict::Reject.as_str(), "reject");
}

#[test]
fn gate_verdict_as_str_skipped() {
    assert_eq!(GateVerdict::Skipped.as_str(), "skipped");
}

#[test]
fn gate_verdict_equality() {
    assert_eq!(GateVerdict::Allow, GateVerdict::Allow);
    assert_ne!(GateVerdict::Allow, GateVerdict::Reject);
    assert_ne!(GateVerdict::Reject, GateVerdict::Skipped);
}

#[test]
fn gate_verdict_copy_clone() {
    let v = GateVerdict::Allow;
    let c = v; // Copy
    let cc = v.clone();
    assert_eq!(v, c);
    assert_eq!(v, cc);
}

// ---------------------------------------------------------------------------
// GateDecision
// ---------------------------------------------------------------------------

fn dec(verdict: GateVerdict, flaw: usize, critical: usize) -> GateDecision {
    GateDecision {
        sop_id: "sop-1".to_string(),
        verdict,
        flaw_count: flaw,
        critical_count: critical,
        confidence: 0.8,
        elapsed_micros: 1234,
    }
}

#[test]
fn gate_decision_fields() {
    let d = dec(GateVerdict::Allow, 2, 1);
    assert_eq!(d.sop_id, "sop-1");
    assert_eq!(d.verdict, GateVerdict::Allow);
    assert_eq!(d.flaw_count, 2);
    assert_eq!(d.critical_count, 1);
    assert!((d.confidence - 0.8).abs() < 1e-9);
    assert_eq!(d.elapsed_micros, 1234);
}

// ---------------------------------------------------------------------------
// GateHistory
// ---------------------------------------------------------------------------

#[test]
fn gate_history_default_empty() {
    let h = GateHistory::new();
    assert!(h.snapshot().is_empty());
}

#[test]
fn gate_history_default_trait() {
    let h = GateHistory::default();
    assert!(h.snapshot().is_empty());
}

#[test]
fn gate_history_record_appends() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Reject, 3, 1));
    h.record(dec(GateVerdict::Skipped, 0, 0));
    let snap = h.snapshot();
    assert_eq!(snap.len(), 3);
    assert_eq!(snap[0].verdict, GateVerdict::Allow);
    assert_eq!(snap[1].verdict, GateVerdict::Reject);
    assert_eq!(snap[2].verdict, GateVerdict::Skipped);
}

#[test]
fn gate_history_intercept_rate_no_decisions() {
    let h = GateHistory::new();
    assert_eq!(h.intercept_rate(), 0.0);
}

#[test]
fn gate_history_intercept_rate_all_allow() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Allow, 0, 0));
    assert_eq!(h.intercept_rate(), 0.0);
}

#[test]
fn gate_history_intercept_rate_all_reject() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Reject, 1, 1));
    h.record(dec(GateVerdict::Reject, 1, 1));
    assert_eq!(h.intercept_rate(), 1.0);
}

#[test]
fn gate_history_intercept_rate_mixed() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Reject, 1, 0));
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Reject, 1, 0));
    // 2 rejects out of 4 (allow + reject).
    assert!((h.intercept_rate() - 0.5).abs() < 1e-9);
}

#[test]
fn gate_history_intercept_rate_excludes_skipped() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Reject, 1, 0));
    h.record(dec(GateVerdict::Skipped, 0, 0));
    h.record(dec(GateVerdict::Skipped, 0, 0));
    // 1 reject out of 2 (allow + reject); skipped excluded.
    assert!((h.intercept_rate() - 0.5).abs() < 1e-9);
}

#[test]
fn gate_history_intercept_rate_only_skipped() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Skipped, 0, 0));
    h.record(dec(GateVerdict::Skipped, 0, 0));
    // No allow + no reject → 0.0 (no division).
    assert_eq!(h.intercept_rate(), 0.0);
}

#[test]
fn gate_history_false_positive_rate_always_zero() {
    let h = GateHistory::new();
    // No decisions → still 0.0.
    assert_eq!(h.false_positive_rate(), 0.0);
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Reject, 1, 1));
    // Implementation note: hook is always 0.0 today.
    assert_eq!(h.false_positive_rate(), 0.0);
}

#[test]
fn gate_history_snapshot_is_clone() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    let s1 = h.snapshot();
    let s2 = h.snapshot();
    assert_eq!(s1.len(), 1);
    assert_eq!(s2.len(), 1);
    // Both snapshots are independent Vec clones.
    assert_eq!(s1[0].verdict, GateVerdict::Allow);
    assert_eq!(s2[0].verdict, GateVerdict::Allow);
}

// ---------------------------------------------------------------------------
// GateConfig
// ---------------------------------------------------------------------------

#[test]
fn gate_config_default() {
    let c = GateConfig::default();
    // Sanity: defaults are set (we don't pin exact values, just that
    // they exist and are accessible).
    let _ = c.enabled;
    let _ = c.fallback_on_error;
}

#[test]
fn gate_config_field_assignment() {
    let c = GateConfig {
        enabled: false,
        fallback_on_error: true,
    };
    assert!(!c.enabled);
    assert!(c.fallback_on_error);
}
