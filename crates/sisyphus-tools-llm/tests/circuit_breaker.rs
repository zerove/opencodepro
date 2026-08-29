//! Integration tests for sisyphus-tools-llm `circuit_breaker`.
//!
//! Exercises the public [`CircuitBreaker`] state machine:
//! `Closed → Open → HalfOpen → Closed` transitions driven by
//! `on_success` / `on_failure` and `check_state`. The breaker
//! protects downstream calls from cascading failures.

use sisyphus_tools_llm::circuit_breaker::{BreakerConfig, BreakerMetrics, CircuitBreaker};
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

// ---------------------------------------------------------------------------
// Additional: BreakerConfig + state per key + call/reset/metrics
// ---------------------------------------------------------------------------

#[test]
fn cb_breaker_config_default_values() {
    let c = BreakerConfig::default();
    assert_eq!(c.failure_threshold, 5);
    assert_eq!(c.success_threshold, 3);
    assert_eq!(c.timeout_secs, 30);
    assert_eq!(c.half_open_max_calls, 3);
}

#[test]
fn cb_breaker_config_field_access() {
    let c = BreakerConfig {
        failure_threshold: 10,
        success_threshold: 2,
        timeout_secs: 60,
        half_open_max_calls: 5,
    };
    assert_eq!(c.failure_threshold, 10);
    assert_eq!(c.success_threshold, 2);
    assert_eq!(c.timeout_secs, 60);
    assert_eq!(c.half_open_max_calls, 5);
}

#[test]
fn cb_breaker_config_serde() {
    let c = BreakerConfig::default();
    let j = serde_json::to_string(&c).unwrap();
    let back: BreakerConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.failure_threshold, c.failure_threshold);
    assert_eq!(back.timeout_secs, c.timeout_secs);
}

#[test]
fn cb_circuit_breaker_default_trait() {
    let cb = CircuitBreaker::default();
    assert_eq!(cb.state("k"), CircuitState::Closed);
    assert_eq!(cb.metrics("k").state, CircuitState::Closed);
}

#[test]
fn cb_circuit_breaker_state_distinct_keys() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    assert_eq!(cb.state("a"), CircuitState::Closed);
    assert_eq!(cb.state("b"), CircuitState::Closed);
    assert_eq!(cb.state("c"), CircuitState::Closed);
}

#[test]
fn cb_circuit_breaker_call_succeeds() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    let r: Result<i32, String> = cb.call("k1", || Ok::<i32, &str>(42));
    assert_eq!(r.unwrap(), 42);
}

#[test]
fn cb_circuit_breaker_call_propagates_error() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    let r: Result<i32, String> = cb.call("k2", || Err::<i32, _>("boom"));
    assert_eq!(r.unwrap_err(), "boom");
}

#[test]
fn cb_circuit_breaker_call_metrics_on_success() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    let _: Result<i32, String> = cb.call("k3", || Ok::<i32, &str>(1));
    let m = cb.metrics("k3");
    // In Closed state, on_success resets failure_count to 0 (not incrementing success_count).
    assert_eq!(m.state, CircuitState::Closed);
    assert_eq!(m.failure_count, 0);
}

#[test]
fn cb_circuit_breaker_call_metrics_on_failure() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    let _: Result<i32, String> = cb.call("k4", || Err::<i32, _>("e"));
    let m = cb.metrics("k4");
    assert_eq!(m.failure_count, 1);
}

#[test]
fn cb_circuit_breaker_reset_clears_metrics() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    let _: Result<i32, String> = cb.call("k5", || Err::<i32, _>("e"));
    assert_eq!(cb.metrics("k5").failure_count, 1);
    cb.reset("k5");
    assert_eq!(cb.metrics("k5").failure_count, 0);
    assert_eq!(cb.state("k5"), CircuitState::Closed);
}

#[test]
fn cb_circuit_breaker_reset_unknown_key_is_noop() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    cb.reset("nonexistent");
    assert_eq!(cb.state("nonexistent"), CircuitState::Closed);
}

#[test]
fn cb_circuit_breaker_reset_all() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    let _: Result<i32, String> = cb.call("a", || Err::<i32, _>("x"));
    let _: Result<i32, String> = cb.call("b", || Err::<i32, _>("x"));
    assert_eq!(cb.metrics("a").failure_count, 1);
    assert_eq!(cb.metrics("b").failure_count, 1);
    cb.reset_all();
    assert_eq!(cb.metrics("a").failure_count, 0);
    assert_eq!(cb.metrics("b").failure_count, 0);
}

#[test]
fn cb_circuit_breaker_metrics_unknown_key() {
    let cb = CircuitBreaker::new(BreakerConfig::default());
    let m = cb.metrics("never");
    assert_eq!(m.state, CircuitState::Closed);
    assert_eq!(m.failure_count, 0);
    assert_eq!(m.success_count, 0);
}

#[test]
fn cb_breaker_metrics_field_access() {
    let m = BreakerMetrics {
        state: CircuitState::HalfOpen,
        failure_count: 2,
        success_count: 1,
    };
    assert_eq!(m.state, CircuitState::HalfOpen);
    assert_eq!(m.failure_count, 2);
    assert_eq!(m.success_count, 1);
}
