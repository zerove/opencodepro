//! Integration tests for sisyphus-intel `token_budget`.
//!
//! Exercises the public [`TokenBudgetTracker`] API. The tracker counts
//! cumulative LLM token usage and exposes a reserve ratio. When
//! remaining budget falls below the reserve, `should_skip_llm`
//! returns true so downstream stages can fall back to deterministic
//! logic.

use sisyphus_intel::token_budget::{TokenBudgetTracker, DEFAULT_ESTIMATED_TOKENS_PER_CALL};

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn new_starts_with_zero_usage() {
    let t = TokenBudgetTracker::new(1000, 0.1);
    assert_eq!(t.usage(), 0);
    assert_eq!(t.remaining(), 1000);
}

#[test]
fn new_clamps_reserve_ratio_above_one() {
    // reserve_ratio = 2.0 → clamped to 1.0.
    // With reserve = cap, any non-zero usage puts remaining below reserve.
    let t = TokenBudgetTracker::new(100, 2.0);
    t.record(1);
    assert!(t.should_skip_llm());
}

#[test]
fn new_clamps_reserve_ratio_below_zero() {
    // reserve_ratio = -1.0 → clamped to 0.0.
    // With reserve = 0, only exhaustion triggers skip.
    let t = TokenBudgetTracker::new(100, -1.0);
    t.record(50);
    assert!(!t.should_skip_llm());
}

#[test]
fn unbounded_never_skips() {
    let t = TokenBudgetTracker::unbounded();
    t.record(10_000_000);
    assert!(!t.should_skip_llm());
    assert!(!t.is_exhausted());
}

// ---------------------------------------------------------------------------
// record / usage / remaining
// ---------------------------------------------------------------------------

#[test]
fn record_accumulates_usage() {
    let t = TokenBudgetTracker::new(1000, 0.0);
    t.record(100);
    t.record(200);
    t.record(300);
    assert_eq!(t.usage(), 600);
}

#[test]
fn record_saturates_at_cap() {
    let t = TokenBudgetTracker::new(100, 0.0);
    t.record(150);
    assert_eq!(t.usage(), 150);
    // remaining saturates at 0 (not underflow).
    assert_eq!(t.remaining(), 0);
}

#[test]
fn remaining_decreases_as_usage_grows() {
    let t = TokenBudgetTracker::new(100, 0.0);
    assert_eq!(t.remaining(), 100);
    t.record(25);
    assert_eq!(t.remaining(), 75);
    t.record(50);
    assert_eq!(t.remaining(), 25);
}

// ---------------------------------------------------------------------------
// should_skip_llm
// ---------------------------------------------------------------------------

#[test]
fn should_skip_below_reserve_threshold() {
    // cap=1000, reserve=0.1 → reserve = 100 tokens
    let t = TokenBudgetTracker::new(1000, 0.1);
    assert!(!t.should_skip_llm());
    t.record(800);
    // 200 remaining, > 100 reserve → no skip
    assert!(!t.should_skip_llm());
    t.record(150);
    // 50 remaining, < 100 reserve → skip
    assert!(t.should_skip_llm());
}

#[test]
fn should_skip_zero_reserve_never_triggers_via_remaining() {
    // With reserve = 0, `remaining < reserve` is only true when
    // remaining is 0 (impossible) — so should_skip_llm always returns false.
    let t = TokenBudgetTracker::new(100, 0.0);
    t.record(50);
    assert!(!t.should_skip_llm());
    t.record(60);
    assert!(!t.should_skip_llm());
}

// ---------------------------------------------------------------------------
// is_exhausted
// ---------------------------------------------------------------------------

#[test]
fn is_exhausted_when_usage_reaches_cap() {
    let t = TokenBudgetTracker::new(100, 0.0);
    t.record(99);
    assert!(!t.is_exhausted());
    t.record(1);
    assert!(t.is_exhausted());
}

#[test]
fn is_exhausted_handles_overflow() {
    let t = TokenBudgetTracker::new(100, 0.0);
    t.record(150);
    // Usage exceeds cap; still "exhausted".
    assert!(t.is_exhausted());
}

// ---------------------------------------------------------------------------
// reset
// ---------------------------------------------------------------------------

#[test]
fn reset_zeros_usage() {
    let t = TokenBudgetTracker::new(100, 0.0);
    t.record(80);
    assert_eq!(t.usage(), 80);
    t.reset();
    assert_eq!(t.usage(), 0);
    assert_eq!(t.remaining(), 100);
}

#[test]
fn reset_clears_skip_state() {
    let t = TokenBudgetTracker::new(100, 0.5);
    t.record(80);
    assert!(t.should_skip_llm());
    t.reset();
    assert!(!t.should_skip_llm());
}

// ---------------------------------------------------------------------------
// Concurrency
// ---------------------------------------------------------------------------

#[test]
fn concurrent_record_is_thread_safe() {
    use std::sync::Arc;
    use std::thread;
    let t = Arc::new(TokenBudgetTracker::new(100_000, 0.0));
    let mut handles = Vec::new();
    for _ in 0..10 {
        let t = t.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..1000 {
                t.record(10);
            }
        }));
    }
    for h in handles {
        h.join().expect("join");
    }
    // 10 threads × 1000 records × 10 tokens = 100_000.
    assert_eq!(t.usage(), 100_000);
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

#[test]
fn default_estimated_tokens_per_call_is_sensible() {
    // The default estimate is used when LLM call sites don't know
    // the exact token count; verify it's positive and not absurdly large.
    const _: () = assert!(DEFAULT_ESTIMATED_TOKENS_PER_CALL > 0);
    const _: () = assert!(DEFAULT_ESTIMATED_TOKENS_PER_CALL < 100_000);
}

// ---------------------------------------------------------------------------
// Additional: per-state tests + reset + clamp
// ---------------------------------------------------------------------------

#[test]
fn tb_record_increments_usage() {
    let t = TokenBudgetTracker::new(1000, 0.0);
    t.record(100);
    assert_eq!(t.usage(), 100);
    t.record(200);
    assert_eq!(t.usage(), 300);
}

#[test]
fn tb_record_saturating_add() {
    let t = TokenBudgetTracker::new(1000, 0.0);
    t.record(u64::MAX);
    t.record(1);
    assert_eq!(t.usage(), u64::MAX);
}

#[test]
fn tb_remaining_saturates_at_zero() {
    let t = TokenBudgetTracker::new(100, 0.0);
    t.record(200);
    assert_eq!(t.remaining(), 0);
}

#[test]
fn tb_should_skip_llm_when_remaining_below_reserve() {
    let t = TokenBudgetTracker::new(1000, 0.1);
    t.record(950);
    assert!(t.should_skip_llm());
}

#[test]
fn tb_should_not_skip_when_remaining_above_reserve() {
    let t = TokenBudgetTracker::new(1000, 0.1);
    t.record(500);
    assert!(!t.should_skip_llm());
}

#[test]
fn tb_is_exhausted_when_usage_equals_cap() {
    let t = TokenBudgetTracker::new(100, 0.0);
    t.record(100);
    assert!(t.is_exhausted());
}

#[test]
fn tb_is_not_exhausted_under_cap() {
    let t = TokenBudgetTracker::new(100, 0.0);
    t.record(99);
    assert!(!t.is_exhausted());
}

#[test]
fn tb_unbounded_never_exhausted() {
    let t = TokenBudgetTracker::unbounded();
    t.record(1_000_000);
    assert!(!t.is_exhausted());
    assert!(!t.should_skip_llm());
}

#[test]
fn tb_reset_clears_usage() {
    let t = TokenBudgetTracker::new(1000, 0.0);
    t.record(500);
    assert_eq!(t.usage(), 500);
    t.reset();
    assert_eq!(t.usage(), 0);
    assert_eq!(t.remaining(), 1000);
}

#[test]
fn tb_reserve_ratio_clamps_negative() {
    let t = TokenBudgetTracker::new(1000, -1.0);
    assert!(!t.should_skip_llm());
}

#[test]
fn tb_reserve_ratio_clamps_above_one() {
    let t = TokenBudgetTracker::new(1000, 5.0);
    t.record(1);
    assert!(t.should_skip_llm());
}

#[test]
fn tb_zero_cap_immediately_exhausted() {
    let t = TokenBudgetTracker::new(0, 0.0);
    assert_eq!(t.remaining(), 0);
    assert!(t.is_exhausted());
}

#[test]
fn tb_debug_format() {
    let t = TokenBudgetTracker::new(1000, 0.0);
    let s = format!("{t:?}");
    assert!(s.contains("TokenBudgetTracker"));
    assert!(s.contains("1000"));
}
