//! Integration tests for sisyphus-mem skill_market RateLimiter.
//!
//! Exercises the public token-bucket rate limiter used to throttle
//! registry requests. Pure, synchronous, and deterministic enough to
//! test from `tests/`.

use sisyphus_mem::skill_market::RateLimiter;
use std::thread;
use std::time::Duration;

#[test]
fn rate_zero_disables_throttling() {
    let mut rl = RateLimiter::new(0);
    // 1000 acquires should all return None (no waiting) when rate=0
    for _ in 0..1000 {
        assert!(
            rl.try_acquire().is_none(),
            "rate=0 should never ask to wait"
        );
    }
}

#[test]
fn fresh_bucket_starts_full_and_drains() {
    let mut rl = RateLimiter::new(3);
    // Initial bucket has 3 tokens → 3 immediate acquires succeed
    assert!(rl.try_acquire().is_none());
    assert!(rl.try_acquire().is_none());
    assert!(rl.try_acquire().is_none());
    // 4th must request a wait
    assert!(rl.try_acquire().is_some());
}

#[test]
fn post_wait_consume_drains_one_token() {
    let mut rl = RateLimiter::new(2);
    assert!(rl.try_acquire().is_none());
    assert!(rl.try_acquire().is_none());
    // Bucket is empty
    assert!(rl.try_acquire().is_some());
    // Caller sleeps then consumes
    thread::sleep(Duration::from_millis(5));
    rl.post_wait_consume();
    // Bucket should still be empty (post_wait_consume is dec-only,
    // it does not refill), so the next acquire is also a wait.
    assert!(rl.try_acquire().is_some());
}

#[test]
fn post_wait_consume_saturates_at_zero() {
    let mut rl = RateLimiter::new(0); // disabled
    // Even when disabled, post_wait_consume must not underflow.
    for _ in 0..100 {
        rl.post_wait_consume();
    }
    // Bucket is conceptually "0" or "infinite" — either way, no panic.
}

#[test]
fn bucket_refills_after_sleep() {
    let mut rl = RateLimiter::new(10); // 10 per second
    // Drain the bucket
    for _ in 0..10 {
        assert!(rl.try_acquire().is_none());
    }
    assert!(rl.try_acquire().is_some());
    // Sleep 200ms → at 10/s, ~2 tokens should be available
    thread::sleep(Duration::from_millis(200));
    assert!(rl.try_acquire().is_none(), "should refill at least 1 token");
    assert!(
        rl.try_acquire().is_none(),
        "should refill at least 2 tokens"
    );
    // 3rd after 200ms might be a wait depending on rounding; just check
    // it doesn't crash.
    let _ = rl.try_acquire();
}

#[test]
fn rate_one_yields_at_most_one_token_per_second() {
    let mut rl = RateLimiter::new(1);
    // Drain
    assert!(rl.try_acquire().is_none());
    // Now empty, must request a wait of ~1000ms
    let wait = rl.try_acquire().expect("should request wait");
    assert!(
        wait >= Duration::from_millis(900) && wait <= Duration::from_millis(1100),
        "rate=1 should yield wait ≈ 1s, got {wait:?}"
    );
}
