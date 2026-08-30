//! 集成测试：simulation_gate（门控决策、统计、配置）
//!
//! 覆盖 `sisyphus_intel::simulation_gate` 中的纯数据/同步模块：
//! - `GateDecision` 字段验证
//! - `GateVerdict::as_str` 三种分支
//! - `GateHistory` record / snapshot / intercept_rate（空 + 混合）
//! - `GateConfig` 默认值

use sisyphus_intel::simulation_gate::{GateConfig, GateDecision, GateHistory, GateVerdict};
use std::sync::Arc;

fn dec(verdict: GateVerdict, flaw: usize, crit: usize) -> GateDecision {
    GateDecision {
        sop_id: format!("sop-{verdict:?}-{flaw}-{crit}"),
        verdict,
        flaw_count: flaw,
        critical_count: crit,
        confidence: 0.9,
        elapsed_micros: 100,
    }
}

// ---------------------------------------------------------------------------
// GateVerdict::as_str
// ---------------------------------------------------------------------------

#[test]
fn verdict_as_str_allow() {
    assert_eq!(GateVerdict::Allow.as_str(), "allow");
}

#[test]
fn verdict_as_str_reject() {
    assert_eq!(GateVerdict::Reject.as_str(), "reject");
}

#[test]
fn verdict_as_str_skipped() {
    assert_eq!(GateVerdict::Skipped.as_str(), "skipped");
}

#[test]
fn verdict_copy_eq() {
    let v = GateVerdict::Allow;
    let v2 = v; // Copy
    assert_eq!(v, v2);
}

// ---------------------------------------------------------------------------
// GateDecision
// ---------------------------------------------------------------------------

#[test]
fn decision_field_access() {
    let d = dec(GateVerdict::Reject, 3, 2);
    assert_eq!(d.verdict, GateVerdict::Reject);
    assert_eq!(d.flaw_count, 3);
    assert_eq!(d.critical_count, 2);
    assert_eq!(d.confidence, 0.9);
    assert!(d.sop_id.contains("Reject"));
}

#[test]
fn decision_clone_eq() {
    let d = dec(GateVerdict::Allow, 0, 0);
    let d2 = d.clone();
    assert_eq!(d, d2);
}

#[test]
fn decision_debug() {
    let d = dec(GateVerdict::Allow, 1, 0);
    let s = format!("{d:?}");
    assert!(s.contains("GateDecision"));
    assert!(s.contains("Allow"));
}

// ---------------------------------------------------------------------------
// GateHistory
// ---------------------------------------------------------------------------

#[test]
fn history_default_empty() {
    let h = GateHistory::default();
    assert!(h.snapshot().is_empty());
    assert_eq!(h.intercept_rate(), 0.0);
    assert_eq!(h.false_positive_rate(), 0.0);
}

#[test]
fn history_new_same_as_default() {
    let h = GateHistory::new();
    assert!(h.snapshot().is_empty());
}

#[test]
fn history_record_one() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    assert_eq!(h.snapshot().len(), 1);
    assert_eq!(h.snapshot()[0].verdict, GateVerdict::Allow);
}

#[test]
fn history_record_many() {
    let h = GateHistory::new();
    for _ in 0..10 {
        h.record(dec(GateVerdict::Allow, 0, 0));
    }
    assert_eq!(h.snapshot().len(), 10);
}

#[test]
fn history_intercept_rate_only_allow() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Allow, 0, 0));
    assert_eq!(h.intercept_rate(), 0.0);
}

#[test]
fn history_intercept_rate_only_reject() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Reject, 1, 1));
    h.record(dec(GateVerdict::Reject, 2, 1));
    assert_eq!(h.intercept_rate(), 1.0);
}

#[test]
fn history_intercept_rate_mixed() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Reject, 1, 1));
    h.record(dec(GateVerdict::Reject, 2, 1));
    // 2 allow, 2 reject → 0.5
    assert!((h.intercept_rate() - 0.5).abs() < 1e-9);
}

#[test]
fn history_intercept_rate_skipped_excluded() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Skipped, 0, 0));
    h.record(dec(GateVerdict::Skipped, 0, 0));
    // only 1 allow in denom → 0.0
    assert_eq!(h.intercept_rate(), 0.0);
}

#[test]
fn history_intercept_rate_only_skipped_is_zero() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Skipped, 0, 0));
    assert_eq!(h.intercept_rate(), 0.0);
}

#[test]
fn history_false_positive_rate_always_zero() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 5, 5));
    h.record(dec(GateVerdict::Reject, 5, 5));
    assert_eq!(h.false_positive_rate(), 0.0);
}

#[test]
fn history_snapshot_independence() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    let s1 = h.snapshot();
    h.record(dec(GateVerdict::Reject, 1, 1));
    let s2 = h.snapshot();
    // s1 captured at len=1, s2 at len=2
    assert_eq!(s1.len(), 1);
    assert_eq!(s2.len(), 2);
}

#[test]
fn history_thread_safe() {
    let h = Arc::new(GateHistory::new());
    let mut handles = vec![];
    for _ in 0..5 {
        let h2 = h.clone();
        handles.push(std::thread::spawn(move || {
            for _ in 0..20 {
                h2.record(dec(GateVerdict::Allow, 0, 0));
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(h.snapshot().len(), 100);
}

// ---------------------------------------------------------------------------
// GateConfig
// ---------------------------------------------------------------------------

#[test]
fn gate_config_default_enabled_fallback() {
    let c = GateConfig::default();
    assert!(c.enabled);
    assert!(c.fallback_on_error);
}

#[test]
fn gate_config_clone() {
    let c = GateConfig {
        enabled: false,
        fallback_on_error: false,
    };
    let c2 = c.clone();
    assert!(!c2.enabled);
    assert!(!c2.fallback_on_error);
}

#[test]
fn gate_config_debug() {
    let c = GateConfig::default();
    let s = format!("{c:?}");
    assert!(s.contains("GateConfig"));
    assert!(s.contains("enabled"));
    assert!(s.contains("fallback_on_error"));
}
