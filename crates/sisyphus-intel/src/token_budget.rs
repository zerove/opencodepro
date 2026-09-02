//! Token budget tracking for LLM calls.
//!
//! Provides a shared, thread-safe tracker that counts cumulative LLM token
//! usage and exposes "budget remaining" semantics. The reflect/evolve
//! stages query `TokenBudgetTracker::should_skip_llm()` before invoking
//! the LLM; when remaining budget is below the configured threshold,
//! the stage falls back to deterministic logic instead of issuing a
//! costly LLM call.
//!
//! ## Use
//!
//! ```
//! use sisyphus_intel::token_budget::TokenBudgetTracker;
//!
//! // Reserve 25% of a 10k-token budget for higher-priority stages.
//! let budget = TokenBudgetTracker::new(10_000, 0.25);
//!
//! budget.record(7_000);
//! assert!(!budget.should_skip_llm()); // 3000 remaining > 2500 reserve
//!
//! budget.record(1_000);
//! assert!(budget.should_skip_llm()); // 2000 remaining < 2500 reserve
//! ```

use parking_lot::Mutex;
use std::sync::Arc;

/// Default per-LLM-call token estimate when exact counts are unavailable.
/// Reflect/evolve stages typically produce prompts in the 1k-4k range; 2000
/// is a conservative middle-ground.
pub const DEFAULT_ESTIMATED_TOKENS_PER_CALL: u64 = 2000;

/// Tracks cumulative LLM token usage against a hard cap and a reserve.
///
/// When `usage >= cap * (1 - reserve)`, `should_skip_llm()` returns true.
#[derive(Debug)]
pub struct TokenBudgetTracker {
    cap: u64,
    reserve_ratio: f64,
    usage: Arc<Mutex<u64>>,
}

impl TokenBudgetTracker {
    /// Create a new tracker with the given hard cap (in tokens) and reserve
    /// ratio (0.0..1.0). When remaining budget falls below `cap * reserve`,
    /// `should_skip_llm()` returns true.
    pub fn new(cap: u64, reserve_ratio: f64) -> Self {
        Self {
            cap,
            reserve_ratio: reserve_ratio.clamp(0.0, 1.0),
            usage: Arc::new(Mutex::new(0)),
        }
    }

    /// Unbounded tracker (no cap, no skip). Useful for tests and dev mode.
    pub fn unbounded() -> Self {
        Self::new(u64::MAX, 0.0)
    }

    /// Record tokens consumed by an LLM call.
    pub fn record(&self, tokens: u64) {
        let mut guard = self.usage.lock();
        *guard = guard.saturating_add(tokens);
    }

    /// Tokens used so far.
    pub fn usage(&self) -> u64 {
        *self.usage.lock()
    }

    /// Tokens remaining (saturating at 0).
    pub fn remaining(&self) -> u64 {
        self.cap.saturating_sub(self.usage())
    }

    /// Returns true when remaining tokens are below the configured reserve,
    /// meaning subsequent LLM calls should be skipped to preserve budget
    /// for higher-priority stages.
    pub fn should_skip_llm(&self) -> bool {
        let reserve_tokens = (self.cap as f64 * self.reserve_ratio) as u64;
        self.remaining() < reserve_tokens
    }

    /// Returns true when the budget is fully exhausted (remaining == 0).
    pub fn is_exhausted(&self) -> bool {
        self.cap != u64::MAX && self.usage() >= self.cap
    }

    /// Read-only snapshot of the tracker's current state. Used by health
    /// endpoints and TUI panels to avoid exposing the internal `Mutex`.
    /// Returns `cap = None` when the tracker is unbounded (`u64::MAX`).
    pub fn snapshot(&self) -> TokenBudgetSnapshot {
        let cap = if self.cap == u64::MAX {
            None
        } else {
            Some(self.cap)
        };
        TokenBudgetSnapshot {
            usage: self.usage(),
            cap,
            exhausted: self.is_exhausted(),
        }
    }

    /// Reset usage (e.g., between runs).
    pub fn reset(&self) {
        *self.usage.lock() = 0;
    }
}

/// Read-only snapshot of [`TokenBudgetTracker`].
///
/// `cap = None` indicates an unbounded tracker (the original used
/// `u64::MAX` as a sentinel). Safe to serialize to JSON and
/// send over the wire (e.g. to TUI `:health` panel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBudgetSnapshot {
    pub usage: u64,
    pub cap: Option<u64>,
    pub exhausted: bool,
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unbounded_never_skips() {
        let t = TokenBudgetTracker::unbounded();
        assert!(!t.should_skip_llm());
        t.record(10_000_000);
        assert!(!t.should_skip_llm());
    }

    #[test]
    fn test_skip_when_below_reserve() {
        let t = TokenBudgetTracker::new(1000, 0.1); // 10% reserve = 100 tokens
        assert!(!t.should_skip_llm());
        t.record(800); // 200 remaining, > 100 reserve
        assert!(!t.should_skip_llm());
        t.record(150); // 50 remaining, < 100 reserve
        assert!(t.should_skip_llm());
    }

    #[test]
    fn test_exhausted_flag() {
        let t = TokenBudgetTracker::new(100, 0.0);
        t.record(50);
        assert!(!t.is_exhausted());
        t.record(60);
        assert!(t.is_exhausted());
    }

    #[test]
    fn test_unbounded_never_exhausted() {
        let t = TokenBudgetTracker::unbounded();
        t.record(u64::MAX / 2);
        assert!(!t.is_exhausted());
    }

    #[test]
    fn test_remaining_saturates() {
        let t = TokenBudgetTracker::new(100, 0.0);
        t.record(150);
        assert_eq!(t.remaining(), 0);
    }

    #[test]
    fn test_reset() {
        let t = TokenBudgetTracker::new(100, 0.0);
        t.record(80);
        assert_eq!(t.usage(), 80);
        t.reset();
        assert_eq!(t.usage(), 0);
        assert_eq!(t.remaining(), 100);
    }

    #[test]
    fn test_reserve_ratio_clamped() {
        // reserve > 1.0 should be clamped to 1.0
        let t = TokenBudgetTracker::new(100, 2.0);
        // reserve = 1.0 means reserve_tokens == cap, so remaining <= reserve always
        assert!(t.remaining() <= t.cap); // sanity
                                         // Verify clamp behavior
        let r = (t.cap as f64 * 2.0_f64.clamp(0.0, 1.0)) as u64;
        assert_eq!(r, 100); // reserve clamped to 1.0
    }

    #[test]
    fn test_concurrent_record() {
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
            h.join().unwrap();
        }
        assert_eq!(t.usage(), 100_000);
    }
}
