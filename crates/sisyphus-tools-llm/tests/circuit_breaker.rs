//! Integration tests for sisyphus-tools-llm `circuit_breaker`.
//!
//! Exercises the public [`CircuitBreaker`] state machine:
//! `Closed → Open → HalfOpen → Closed` transitions driven by
//! `on_success` / `on_failure` and `check_state`. The breaker
//! protects downstream calls from cascading failures.

use sisyphus_tools_llm::circuit_breaker::{BreakerConfig, CircuitBreaker};
use sisyphus_types::CircuitState;

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn new_breaker_starts_closed() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    assert_eq!(cb.state("any_key"), CircuitState::Closed);
    assert_eq!(cb.state("another"), CircuitState::Closed);
}

#[test]
fn default_breaker_starts_closed() {
    let cb = CircuitBreaker::default();
    assert_eq!(cb.state("svc"), CircuitState::Closed);
}

// ---------------------------------------------------------------------------
// check_state
// ---------------------------------------------------------------------------

#[test]
fn check_state_closed_allows_call() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    assert!(cb.check_state("svc").is_ok());
}

#[test]
fn check_state_unknown_key_initializes_as_closed() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    // Even an unknown key starts in Closed.
    assert!(cb.check_state("never_used").is_ok());
    assert_eq!(cb.state("never_used"), CircuitState::Closed);
}

// ---------------------------------------------------------------------------
// on_failure → Open
// ---------------------------------------------------------------------------

#[test]
fn on_failure_increments_counter() {
    let cb = CircuitBreaker::new(BreakerConfig {
        failure_threshold: 5,
        ..Default::default()
    });
    cb.check_state("svc").expect("initialize");
    cb.on_failure("svc");
    let m = cb.metrics("svc");
    assert_eq!(m.failure_count, 1);
    assert_eq!(m.state, CircuitState::Closed);
}

#[test]
fn on_failure_at_threshold_opens_circuit() {
    let cb = CircuitBreaker::new(BreakerConfig {
        failure_threshold: 3,
        ..Default::default()
    });
    cb.check_state("svc").expect("init");
    cb.on_failure("svc");
    cb.on_failure("svc");
    cb.on_failure("svc");
    assert_eq!(cb.state("svc"), CircuitState::Open);
}

#[test]
fn on_success_in_closed_state_resets_failure_count() {
    let cb = CircuitBreaker::new(BreakerConfig {
        failure_threshold: 5,
        ..Default::default()
    });
    cb.check_state("svc").expect("init");
    cb.on_failure("svc");
    cb.on_failure("svc");
    assert_eq!(cb.metrics("svc").failure_count, 2);
    cb.on_success("svc");
    assert_eq!(cb.metrics("svc").failure_count, 0);
}

// ---------------------------------------------------------------------------
// on_success in HalfOpen
// ---------------------------------------------------------------------------

#[test]
fn on_success_in_half_open_increments_success_count() {
    let cb = CircuitBreaker::new(BreakerConfig {
        failure_threshold: 1,
        success_threshold: 3,
        timeout_secs: 0, // Open → HalfOpen immediately
        ..Default::default()
    });
    cb.check_state("svc").expect("init");
    cb.on_failure("svc");
    // State is now Open; check_state transitions it to HalfOpen.
    assert!(cb.check_state("svc").is_ok());
    assert_eq!(cb.state("svc"), CircuitState::HalfOpen);

    cb.on_success("svc");
    assert_eq!(cb.metrics("svc").success_count, 1);
    // State stays HalfOpen until threshold reached.
    assert_eq!(cb.state("svc"), CircuitState::HalfOpen);
}

#[test]
fn on_success_threshold_closes_half_open_circuit() {
    let cb = CircuitBreaker::new(BreakerConfig {
        failure_threshold: 1,
        success_threshold: 3,
        timeout_secs: 0,
        ..Default::default()
    });
    cb.check_state("svc").expect("init");
    cb.on_failure("svc");
    assert!(cb.check_state("svc").is_ok());
    cb.on_success("svc");
    cb.on_success("svc");
    cb.on_success("svc");
    // After success_threshold consecutive successes → Closed.
    assert_eq!(cb.state("svc"), CircuitState::Closed);
}

// ---------------------------------------------------------------------------
// on_failure in HalfOpen → Open
// ---------------------------------------------------------------------------

#[test]
fn on_failure_in_half_open_reopens_circuit() {
    let cb = CircuitBreaker::new(BreakerConfig {
        failure_threshold: 1,
        success_threshold: 3,
        timeout_secs: 0,
        ..Default::default()
    });
    cb.check_state("svc").expect("init");
    cb.on_failure("svc");
    let _ = cb.check_state("svc");
    assert_eq!(cb.state("svc"), CircuitState::HalfOpen);
    cb.on_failure("svc");
    assert_eq!(cb.state("svc"), CircuitState::Open);
}

// ---------------------------------------------------------------------------
// reset
// ---------------------------------------------------------------------------

#[test]
fn reset_clears_breaker_to_closed() {
    let cb = CircuitBreaker::new(BreakerConfig {
        failure_threshold: 1,
        ..Default::default()
    });
    cb.check_state("svc").expect("init");
    cb.on_failure("svc");
    assert_eq!(cb.state("svc"), CircuitState::Open);
    cb.reset("svc");
    assert_eq!(cb.state("svc"), CircuitState::Closed);
    assert_eq!(cb.metrics("svc").failure_count, 0);
}

#[test]
fn reset_all_clears_all_breakers() {
    let cb = CircuitBreaker::new(BreakerConfig {
        failure_threshold: 1,
        ..Default::default()
    });
    cb.check_state("svc_a").expect("init");
    cb.on_failure("svc_a");
    cb.check_state("svc_b").expect("init");
    cb.on_failure("svc_b");
    assert_eq!(cb.state("svc_a"), CircuitState::Open);
    assert_eq!(cb.state("svc_b"), CircuitState::Open);
    cb.reset_all();
    assert_eq!(cb.state("svc_a"), CircuitState::Closed);
    assert_eq!(cb.state("svc_b"), CircuitState::Closed);
}

// ---------------------------------------------------------------------------
// metrics
// ---------------------------------------------------------------------------

#[test]
fn metrics_for_unknown_key_returns_defaults() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    let m = cb.metrics("never_seen");
    assert_eq!(m.state, CircuitState::Closed);
    assert_eq!(m.failure_count, 0);
    assert_eq!(m.success_count, 0);
}

#[test]
fn metrics_reflects_state_correctly() {
    let cb = CircuitBreaker::new(BreakerConfig {
        failure_threshold: 2,
        success_threshold: 1,
        timeout_secs: 0,
        ..Default::default()
    });
    cb.check_state("svc").expect("init");
    cb.on_failure("svc");
    cb.on_failure("svc");
    let m = cb.metrics("svc");
    assert_eq!(m.state, CircuitState::Open);
    assert_eq!(m.failure_count, 2);
}

// ---------------------------------------------------------------------------
// BreakerConfig
// ---------------------------------------------------------------------------

#[test]
fn breaker_config_default_values() {
    let cfg = BreakerConfig::default();
    assert_eq!(cfg.failure_threshold, 5);
    assert_eq!(cfg.success_threshold, 3);
    assert_eq!(cfg.timeout_secs, 30);
    assert_eq!(cfg.half_open_max_calls, 3);
}

#[test]
fn breaker_config_serialization_roundtrip() {
    let cfg = BreakerConfig {
        failure_threshold: 7,
        success_threshold: 4,
        timeout_secs: 60,
        half_open_max_calls: 5,
    };
    let json = serde_json::to_string(&cfg).expect("");
    let parsed: BreakerConfig = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.failure_threshold, 7);
    assert_eq!(parsed.success_threshold, 4);
    assert_eq!(parsed.timeout_secs, 60);
    assert_eq!(parsed.half_open_max_calls, 5);
}
