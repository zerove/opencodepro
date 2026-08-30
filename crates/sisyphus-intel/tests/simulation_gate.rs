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

// ---------------------------------------------------------------------------
// GateVerdict additional coverage
// ---------------------------------------------------------------------------

#[test]
fn gate_verdict_debug_format() {
    let _ = format!("{:?}", GateVerdict::Allow);
    let _ = format!("{:?}", GateVerdict::Reject);
    let _ = format!("{:?}", GateVerdict::Skipped);
}

#[test]
fn gate_verdict_clone() {
    let a = GateVerdict::Allow;
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn gate_decision_debug_format() {
    let d = dec(GateVerdict::Allow, 0, 0);
    let dbg = format!("{:?}", d);
    assert!(dbg.contains("GateDecision"));
    assert!(dbg.contains("sop_id"));
}

#[test]
fn gate_decision_unicode_sop_id() {
    let d = GateDecision {
        sop_id: "图-α".into(),
        verdict: GateVerdict::Allow,
        flaw_count: 0,
        critical_count: 0,
        confidence: 0.99,
        elapsed_micros: 1234,
    };
    assert_eq!(d.sop_id, "图-α");
    let cloned = d.clone();
    assert_eq!(cloned.sop_id, "图-α");
}

#[test]
fn gate_decision_high_flaw_count() {
    let d = GateDecision {
        sop_id: "p".into(),
        verdict: GateVerdict::Reject,
        flaw_count: 1000,
        critical_count: 50,
        confidence: 0.1,
        elapsed_micros: u128::MAX,
    };
    assert_eq!(d.flaw_count, 1000);
    assert_eq!(d.critical_count, 50);
    assert_eq!(d.elapsed_micros, u128::MAX);
}

#[test]
fn gate_decision_partial_eq_self() {
    let d = dec(GateVerdict::Allow, 0, 0);
    assert_eq!(d, d.clone());
}

#[test]
fn gate_decision_partial_eq_distinct() {
    let d1 = dec(GateVerdict::Allow, 0, 0);
    let d2 = dec(GateVerdict::Reject, 1, 1);
    assert_ne!(d1, d2);
}

#[test]
fn gate_history_clone_via_default() {
    let h1 = GateHistory::default();
    let h2 = GateHistory::new();
    // Both empty, but distinct; we just verify they don't panic on construction
    let _ = h1.snapshot();
    let _ = h2.snapshot();
}

#[test]
fn gate_history_intercept_rate_one_decision_allow() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    assert_eq!(h.intercept_rate(), 0.0);
    assert_eq!(h.false_positive_rate(), 0.0);
}

#[test]
fn gate_history_intercept_rate_one_decision_reject() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Reject, 1, 1));
    assert_eq!(h.intercept_rate(), 1.0);
}

#[test]
fn gate_history_false_positive_rate_with_reject() {
    // FPR is always 0 by design (no labeled ground truth)
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Reject, 5, 3));
    assert_eq!(h.false_positive_rate(), 0.0);
}

#[test]
fn gate_history_snapshot_empty() {
    let h = GateHistory::new();
    let s = h.snapshot();
    assert!(s.is_empty());
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn gate_verdict_debug_distinct() {
    // Each variant's Debug output should be distinct
    let s_allow = format!("{:?}", GateVerdict::Allow);
    let s_reject = format!("{:?}", GateVerdict::Reject);
    let s_skipped = format!("{:?}", GateVerdict::Skipped);
    assert_ne!(s_allow, s_reject);
    assert_ne!(s_reject, s_skipped);
    assert_ne!(s_allow, s_skipped);
}

#[test]
fn gate_decision_with_max_confidence() {
    let d = GateDecision {
        sop_id: "p".into(),
        verdict: GateVerdict::Allow,
        flaw_count: 0,
        critical_count: 0,
        confidence: 1.0,
        elapsed_micros: 0,
    };
    assert_eq!(d.confidence, 1.0);
    let cloned = d.clone();
    assert_eq!(cloned.confidence, 1.0);
}

#[test]
fn gate_decision_with_zero_confidence() {
    let d = GateDecision {
        sop_id: "p".into(),
        verdict: GateVerdict::Reject,
        flaw_count: 100,
        critical_count: 50,
        confidence: 0.0,
        elapsed_micros: 1000,
    };
    assert_eq!(d.confidence, 0.0);
}

#[test]
fn gate_decision_with_max_u128_elapsed() {
    let d = GateDecision {
        sop_id: "p".into(),
        verdict: GateVerdict::Allow,
        flaw_count: 0,
        critical_count: 0,
        confidence: 0.5,
        elapsed_micros: u128::MAX,
    };
    let j = format!("{:?}", d);
    assert!(j.contains("GateDecision"));
    let cloned = d.clone();
    assert_eq!(cloned.elapsed_micros, u128::MAX);
}

#[test]
fn gate_history_with_skipped_only() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Skipped, 0, 0));
    h.record(dec(GateVerdict::Skipped, 0, 0));
    // Skipped doesn't count in intercept_rate
    assert_eq!(h.intercept_rate(), 0.0);
    assert_eq!(h.snapshot().len(), 2);
}

#[test]
fn gate_history_skipped_doesnt_count_in_denominator() {
    let h = GateHistory::new();
    h.record(dec(GateVerdict::Allow, 0, 0));
    h.record(dec(GateVerdict::Reject, 1, 1));
    h.record(dec(GateVerdict::Skipped, 0, 0));
    h.record(dec(GateVerdict::Skipped, 0, 0));
    // 1 reject out of 2 (allow+reject) = 0.5
    assert!((h.intercept_rate() - 0.5).abs() < 1e-9);
}

#[test]
fn gate_history_mixed_high_volume() {
    let h = GateHistory::new();
    for _ in 0..100 {
        h.record(dec(GateVerdict::Allow, 0, 0));
    }
    for _ in 0..50 {
        h.record(dec(GateVerdict::Reject, 1, 1));
    }
    // 50 reject out of 150 (allow+reject) = 1/3
    let rate = h.intercept_rate();
    assert!((rate - (50.0 / 150.0)).abs() < 1e-9);
    assert_eq!(h.snapshot().len(), 150);
}

#[test]
fn gate_config_field_assignment() {
    let c = GateConfig {
        fallback_on_error: false,
        enabled: false,
    };
    assert!(!c.fallback_on_error);
    assert!(!c.enabled);
    let c2 = GateConfig {
        fallback_on_error: true,
        enabled: true,
    };
    assert!(c2.fallback_on_error);
    assert!(c2.enabled);
}

#[test]
fn gate_config_clone_preserves_fields() {
    let c = GateConfig {
        fallback_on_error: false,
        enabled: true,
    };
    let cloned = c.clone();
    assert_eq!(cloned.fallback_on_error, false);
    assert_eq!(cloned.enabled, true);
}

// ---------------------------------------------------------------------------
// Additional concurrency tests
// ---------------------------------------------------------------------------

#[test]
fn gate_history_concurrent_record_does_not_panic() {
    use std::thread;
    let h = GateHistory::new();
    let h_arc = std::sync::Arc::new(h);
    let mut handles = vec![];
    for _ in 0..20 {
        let h_clone = std::sync::Arc::clone(&h_arc);
        let handle = thread::spawn(move || {
            for _ in 0..10 {
                h_clone.record(dec(GateVerdict::Allow, 0, 0));
            }
        });
        handles.push(handle);
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(h_arc.snapshot().len(), 200);
}

#[test]
fn gate_history_concurrent_snapshot_and_record() {
    use std::thread;
    let h = GateHistory::new();
    let h_arc = std::sync::Arc::new(h);
    for _ in 0..50 {
        h_arc.record(dec(GateVerdict::Allow, 0, 0));
    }
    let mut handles = vec![];
    for _ in 0..5 {
        let h_clone = std::sync::Arc::clone(&h_arc);
        let handle = thread::spawn(move || {
            let _ = h_clone.snapshot();
        });
        handles.push(handle);
    }
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(h_arc.snapshot().len(), 50);
}

#[test]
fn gate_decision_with_min_confidence() {
    // Test boundary f64 value
    let d = GateDecision {
        sop_id: "p".into(),
        verdict: GateVerdict::Allow,
        flaw_count: 0,
        critical_count: 0,
        confidence: f64::MIN_POSITIVE,
        elapsed_micros: 0,
    };
    assert!(d.confidence > 0.0);
}

#[test]
fn gate_decision_with_zero_elapsed() {
    let d = GateDecision {
        sop_id: "p".into(),
        verdict: GateVerdict::Allow,
        flaw_count: 0,
        critical_count: 0,
        confidence: 0.5,
        elapsed_micros: 0,
    };
    assert_eq!(d.elapsed_micros, 0);
}

#[test]
fn gate_decision_with_max_flaw_count() {
    let d = GateDecision {
        sop_id: "p".into(),
        verdict: GateVerdict::Reject,
        flaw_count: usize::MAX,
        critical_count: usize::MAX,
        confidence: 0.0,
        elapsed_micros: 1000,
    };
    assert_eq!(d.flaw_count, usize::MAX);
}

#[test]
fn gate_decision_with_unicode_sop_id() {
    let d = GateDecision {
        sop_id: "图-α".into(),
        verdict: GateVerdict::Allow,
        flaw_count: 0,
        critical_count: 0,
        confidence: 0.9,
        elapsed_micros: 100,
    };
    assert_eq!(d.sop_id, "图-α");
}

#[test]
fn gate_config_with_long_unicode() {
    // Config doesn't have any string fields, just bools
    let c = GateConfig {
        fallback_on_error: true,
        enabled: false,
    };
    let dbg = format!("{:?}", c);
    assert!(dbg.contains("fallback_on_error"));
    assert!(dbg.contains("enabled"));
}
