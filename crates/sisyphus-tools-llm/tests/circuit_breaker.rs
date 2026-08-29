//! Integration tests for sisyphus-tools-llm CircuitBreaker.
//!
//! Exercises the public circuit-breaker state machine used to protect
//! the LLM endpoint from cascading failures. The breaker has three
//! states (Closed, Open, HalfOpen) and four transitions driven by
//! `on_success` / `on_failure` / `check_state`.

use sisyphus_tools_llm::circuit_breaker::{BreakerConfig, CircuitBreaker};
use std::thread;
use std::time::Duration;

fn fast_breaker(threshold: u32) -> CircuitBreaker {
    // 0-second timeout so Open→HalfOpen transitions happen on the next call.
    CircuitBreaker::new(BreakerConfig {
        failure_threshold: threshold,
        success_threshold: 2,
        timeout_secs: 0,
        half_open_max_calls: 1,
    })
}

#[test]
fn unknown_key_defaults_to_closed() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    assert_eq!(cb.state("never-seen"), sisyphus_types::CircuitState::Closed);
    assert!(cb.check_state("never-seen").is_ok());
}

#[test]
fn failures_open_the_circuit_at_threshold() {
    let cb = fast_breaker(3);
    let key = "svc-a";

    // 2 failures: still closed
    for _ in 0..2 {
        let _: Result<(), String> = cb.call(key, || Err("boom"));
    }
    assert_eq!(cb.state(key), sisyphus_types::CircuitState::Closed);

    // 3rd failure trips the breaker
    let _: Result<(), String> = cb.call(key, || Err("boom"));
    assert_eq!(cb.state(key), sisyphus_types::CircuitState::Open);
}

#[test]
fn open_circuit_with_long_timeout_rejects_calls() {
    // With a long timeout, the breaker stays Open and rejects the next
    // call before invoking the closure.
    let cb = CircuitBreaker::new(BreakerConfig {
        failure_threshold: 1,
        success_threshold: 1,
        timeout_secs: 3600, // effectively never auto-recover in this test
        half_open_max_calls: 1,
    });
    let key = "svc-b";
    // Trip the breaker
    let _: Result<(), String> = cb.call(key, || Err("first failure"));
    assert_eq!(cb.state(key), sisyphus_types::CircuitState::Open);

    let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let called_clone = called.clone();
    let result: Result<(), String> = cb.call(key, || {
        called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        Err("should not run")
    });
    assert!(result.is_err(), "open circuit should fail the call, got {result:?}");
    assert!(
        !called.load(std::sync::atomic::Ordering::SeqCst),
        "closure must not be invoked when the circuit is open"
    );
}

#[test]
fn success_in_closed_state_keeps_state_closed() {
    // success_count is only meaningful in HalfOpen state — it's the
    // count of consecutive successes leading to Closed. In Closed, the
    // only counter that matters is failure_count, which is reset on
    // every success.
    let cb = fast_breaker(5);
    let key = "svc-c";
    for _ in 0..10 {
        let _: Result<i32, String> = cb.call(key, || -> Result<i32, &str> { Ok(42) });
    }
    assert_eq!(cb.state(key), sisyphus_types::CircuitState::Closed);
    let m = cb.metrics(key);
    assert_eq!(m.failure_count, 0, "no failures should have been recorded");
}

#[test]
fn success_resets_failure_count_in_closed_state() {
    let cb = fast_breaker(3);
    let key = "svc-d";
    let _: Result<(), String> = cb.call(key, || Err("e1"));
    let _: Result<(), String> = cb.call(key, || Err("e2"));
    // One success resets the consecutive-failure count
    let _: Result<i32, String> = cb.call(key, || -> Result<i32, &str> { Ok(1) });
    // Now we need 3 more consecutive failures to trip
    let _: Result<(), String> = cb.call(key, || Err("e3"));
    let _: Result<(), String> = cb.call(key, || Err("e4"));
    assert_eq!(
        cb.state(key),
        sisyphus_types::CircuitState::Closed,
        "success in closed state should reset the failure counter"
    );
}

#[test]
fn reset_returns_to_closed_state() {
    let cb = fast_breaker(1);
    let key = "svc-e";
    let _: Result<(), String> = cb.call(key, || Err("trip"));
    assert_eq!(cb.state(key), sisyphus_types::CircuitState::Open);

    cb.reset(key);
    assert_eq!(cb.state(key), sisyphus_types::CircuitState::Closed);
    // After reset, the breaker is fresh and accepts calls.
    let result: Result<i32, String> = cb.call(key, || -> Result<i32, &str> { Ok(99) });
    assert_eq!(result, Ok(99));
}

#[test]
fn reset_all_clears_every_breaker() {
    let cb = fast_breaker(1);
    for k in ["a", "b", "c"] {
        let _: Result<(), String> = cb.call(k, || Err("trip"));
        assert_eq!(cb.state(k), sisyphus_types::CircuitState::Open);
    }
    cb.reset_all();
    for k in ["a", "b", "c"] {
        assert_eq!(cb.state(k), sisyphus_types::CircuitState::Closed);
    }
}

#[test]
fn call_propagates_inner_value_on_success() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    let result: Result<String, String> = cb.call("svc-f", || -> Result<String, &str> { Ok("hello".to_string()) });
    assert_eq!(result.unwrap(), "hello");
}

#[test]
fn call_propagates_inner_error_message_on_failure() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    let result: Result<(), String> = cb.call("svc-g", || Err("specific error".to_string()));
    let err = result.expect_err("should error");
    assert_eq!(err, "specific error");
}

#[test]
fn metrics_reflect_failure_count_in_closed_state() {
    // In Closed state, success_count is not incremented (it's a
    // HalfOpen-only counter). Only failure_count tracks traffic.
    let cb = fast_breaker(10);
    let key = "svc-h";
    let _: Result<(), String> = cb.call(key, || Err("e"));
    let _: Result<(), String> = cb.call(key, || Err("e"));
    let _: Result<i32, String> = cb.call(key, || -> Result<i32, &str> { Ok(1) });
    let m = cb.metrics(key);
    // 2 failures + 1 success → consecutive failures reset to 0
    assert_eq!(m.failure_count, 0, "success after failures should reset the count");
    assert_eq!(m.state, sisyphus_types::CircuitState::Closed);
}

#[test]
fn half_open_transition_after_timeout() {
    // timeout_secs=0 means Open→HalfOpen is immediate on next check_state.
    let cb = fast_breaker(1);
    let key = "svc-i";
    let _: Result<(), String> = cb.call(key, || Err("trip"));
    assert_eq!(cb.state(key), sisyphus_types::CircuitState::Open);
    // The act of calling again should transition the state.
    thread::sleep(Duration::from_millis(10));
    let _: Result<(), String> = cb.call(key, || Err("e"));
    // After one failure in HalfOpen, it goes back to Open.
    let s = cb.state(key);
    assert!(
        s == sisyphus_types::CircuitState::Open
            || s == sisyphus_types::CircuitState::HalfOpen,
        "expected Open or HalfOpen after timeout + failure, got {s:?}"
    );
}
