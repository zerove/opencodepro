//! Integration tests for sisyphus-intel TokenBudgetTracker.
//!
//! Exercises the public `TokenBudgetTracker` API as an external consumer
//! would: typical SOP-run usage, edge cases, and concurrent recording.
//! The unit tests in `src/token_budget.rs` cover the simple invariants;
//! these tests add scenarios that mirror how the reflect/evolve stages
//! actually drive the budget.

use sisyphus_intel::token_budget::{TokenBudgetTracker, DEFAULT_ESTIMATED_TOKENS_PER_CALL};

#[test]
fn unbounded_never_skips_and_never_exhausts() {
    let t = TokenBudgetTracker::unbounded();
    assert!(!t.should_skip_llm());
    assert!(!t.is_exhausted());
    // Even with a huge record, the unbounded tracker stays open.
    t.record(10_000_000);
    assert!(!t.should_skip_llm());
    assert!(!t.is_exhausted());
    assert!(t.remaining() > 0, "unbounded tracker must have non-zero remaining");
}

#[test]
fn new_clamps_reserve_ratio_to_unit_interval() {
    // reserve > 1.0 should clamp to 1.0 (always below reserve = always skip)
    let t = TokenBudgetTracker::new(100, 5.0);
    t.record(1);
    assert!(t.should_skip_llm(), "clamped ratio=1.0 → always skip");

    // reserve < 0.0 should clamp to 0.0
    let t = TokenBudgetTracker::new(100, -1.0);
    t.record(99);
    assert!(!t.should_skip_llm(), "clamped ratio=0.0 → never skip from reserve");
    assert!(!t.is_exhausted());
}

#[test]
fn skip_threshold_uses_reserve() {
    // 1000 cap, 20% reserve → skip when remaining < 200
    let t = TokenBudgetTracker::new(1000, 0.2);
    assert!(!t.should_skip_llm());
    t.record(799);
    assert!(!t.should_skip_llm(), "remaining=201, still above reserve 200");
    t.record(2);
    assert!(t.should_skip_llm(), "remaining=199, below reserve 200");
}

#[test]
fn record_saturates_on_overflow() {
    let t = TokenBudgetTracker::new(1000, 0.0);
    t.record(u64::MAX);
    // Should not panic; the tracker saturates.
    let used = t.usage();
    assert_eq!(used, u64::MAX, "saturating add should cap at u64::MAX");
    assert!(t.is_exhausted());
}

#[test]
fn reset_clears_usage_but_keeps_cap() {
    let t = TokenBudgetTracker::new(1000, 0.1);
    t.record(500);
    assert_eq!(t.usage(), 500);
    t.reset();
    assert_eq!(t.usage(), 0);
    assert!(!t.should_skip_llm(), "after reset, reserve check is fresh");
    assert!(!t.is_exhausted());
}

#[test]
fn remaining_is_saturating_subtraction() {
    // If somehow usage > cap, remaining should clamp to 0, not wrap.
    let t = TokenBudgetTracker::new(100, 0.0);
    t.record(150);
    assert_eq!(t.remaining(), 0, "remaining must saturate at 0");
    assert!(t.is_exhausted());
}

#[test]
fn default_estimated_tokens_per_call_is_a_positive_constant() {
    // Sanity check: the public constant should be usable as a fallback
    // when exact token counts are unavailable.
    assert!(DEFAULT_ESTIMATED_TOKENS_PER_CALL > 0);
    // A typical prompt fits in a few thousand tokens, so 2000 is sane.
    assert!(DEFAULT_ESTIMATED_TOKENS_PER_CALL <= 100_000);
}

#[test]
fn concurrent_record_does_not_lose_updates() {
    use std::sync::Arc;
    use std::thread;

    let t = Arc::new(TokenBudgetTracker::new(1_000_000, 0.0));
    let threads = 4;
    let per_thread = 1000u64;
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let t = Arc::clone(&t);
            thread::spawn(move || {
                for _ in 0..per_thread {
                    t.record(1);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread join");
    }
    let expected = threads * per_thread;
    assert_eq!(
        t.usage(),
        expected,
        "concurrent record must count all increments exactly"
    );
}
