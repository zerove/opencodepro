use serde::{Deserialize, Serialize};
use sisyphus_types::CircuitState;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Configuration for a single circuit breaker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakerConfig {
    /// Number of consecutive failures before opening the circuit.
    pub failure_threshold: u32,
    /// Number of consecutive successes in half-open state before closing.
    pub success_threshold: u32,
    /// Time in seconds to wait before transitioning from Open to HalfOpen.
    pub timeout_secs: u64,
    /// Maximum number of calls allowed in HalfOpen state.
    pub half_open_max_calls: u32,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout_secs: 30,
            half_open_max_calls: 3,
        }
    }
}

/// Internal state for a single breaker key.
#[derive(Debug, Clone)]
struct BreakerState {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    last_failure_time: Option<Instant>,
    half_open_calls: u32,
}

impl BreakerState {
    fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: None,
            half_open_calls: 0,
        }
    }
}

/// Circuit breaker pattern implementation.
///
/// Protects resources from cascading failures by monitoring call outcomes.
/// States: Closed → Open (on failure_threshold) → HalfOpen (after timeout) → Closed (on success)
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    config: BreakerConfig,
    breakers: Arc<RwLock<HashMap<String, BreakerState>>>,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(BreakerConfig::default())
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration.
    pub fn new(config: BreakerConfig) -> Self {
        Self {
            config,
            breakers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Execute a fallible operation through the circuit breaker.
    ///
    /// Returns Ok(T) on success, or an error string explaining why the call was rejected/failed.
    pub fn call<T, E: ToString>(
        &self,
        key: &str,
        f: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, String> {
        // Check state and transition if needed
        self.check_state(key)?;

        // Attempt the call
        match f() {
            Ok(result) => {
                self.on_success(key);
                Ok(result)
            }
            Err(e) => {
                self.on_failure(key);
                Err(e.to_string())
            }
        }
    }

    /// Get the current state of a breaker.
    pub fn state(&self, key: &str) -> CircuitState {
        self.breakers
            .read()
            .ok()
            .and_then(|b| b.get(key).map(|s| s.state))
            .unwrap_or(CircuitState::Closed)
    }

    /// Reset a breaker to Closed state.
    pub fn reset(&self, key: &str) {
        if let Ok(mut breakers) = self.breakers.write() {
            breakers.insert(key.to_string(), BreakerState::new());
        }
    }

    /// Reset all breakers.
    pub fn reset_all(&self) {
        if let Ok(mut breakers) = self.breakers.write() {
            breakers.clear();
        }
    }

    /// Get metrics about a breaker.
    pub fn metrics(&self, key: &str) -> BreakerMetrics {
        let breakers = self.breakers.read().ok();
        if let Some(guard) = breakers {
            if let Some(state) = guard.get(key) {
                return BreakerMetrics {
                    state: state.state,
                    failure_count: state.failure_count,
                    success_count: state.success_count,
                };
            }
        }
        BreakerMetrics {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
        }
    }

    pub fn check_state(&self, key: &str) -> Result<(), String> {
        let mut breakers = self
            .breakers
            .write()
            .map_err(|e| format!("CircuitBreaker lock poisoned: {e}"))?;

        let state = breakers
            .entry(key.to_string())
            .or_insert_with(BreakerState::new);

        match state.state {
            CircuitState::Closed => Ok(()),

            CircuitState::Open => {
                // Check if timeout has elapsed → transition to HalfOpen
                if let Some(last_fail) = state.last_failure_time {
                    if last_fail.elapsed() >= Duration::from_secs(self.config.timeout_secs) {
                        state.state = CircuitState::HalfOpen;
                        state.half_open_calls = 1;
                        state.success_count = 0;
                        tracing::info!(breaker = %key, "Circuit transitioning Open -> HalfOpen");
                        return Ok(());
                    }
                }
                Err(format!(
                    "Circuit breaker '{}' is OPEN. Retry after {}s timeout.",
                    key, self.config.timeout_secs
                ))
            }

            CircuitState::HalfOpen => {
                if state.half_open_calls >= self.config.half_open_max_calls {
                    return Err(format!(
                        "Circuit breaker '{}' is HALF_OPEN and at max concurrent calls ({}).",
                        key, self.config.half_open_max_calls
                    ));
                }
                state.half_open_calls += 1;
                Ok(())
            }
        }
    }

    pub fn on_success(&self, key: &str) {
        if let Ok(mut breakers) = self.breakers.write() {
            if let Some(state) = breakers.get_mut(key) {
                match state.state {
                    CircuitState::Closed => {
                        // Reset failure count on success in closed state
                        state.failure_count = 0;
                    }
                    CircuitState::HalfOpen => {
                        state.success_count += 1;
                        if state.success_count >= self.config.success_threshold {
                            state.state = CircuitState::Closed;
                            state.failure_count = 0;
                            state.success_count = 0;
                            state.half_open_calls = 0;
                            tracing::info!(breaker = %key, "Circuit transitioning HalfOpen -> Closed");
                        }
                    }
                    CircuitState::Open => {
                        // Shouldn't happen (call was blocked), but handle gracefully
                        tracing::warn!(breaker = %key, "Unexpected success in Open state");
                    }
                }
            }
        }
    }

    pub fn on_failure(&self, key: &str) {
        if let Ok(mut breakers) = self.breakers.write() {
            if let Some(state) = breakers.get_mut(key) {
                state.last_failure_time = Some(Instant::now());
                state.success_count = 0;

                match state.state {
                    CircuitState::Closed => {
                        state.failure_count += 1;
                        if state.failure_count >= self.config.failure_threshold {
                            state.state = CircuitState::Open;
                            tracing::warn!(
                                breaker = %key,
                                failures = state.failure_count,
                                "Circuit transitioning Closed -> Open"
                            );
                        }
                    }
                    CircuitState::HalfOpen => {
                        // Any failure in half-open → back to open
                        state.state = CircuitState::Open;
                        state.half_open_calls = 0;
                        tracing::warn!(breaker = %key, "Circuit transitioning HalfOpen -> Open (failure)");
                    }
                    CircuitState::Open => {
                        // Just update the last failure time
                        tracing::debug!(breaker = %key, "Failure while Open (extending timeout)");
                    }
                }
            }
        }
    }
}

/// Snapshot of breaker metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakerMetrics {
    pub state: CircuitState,
    pub failure_count: u32,
    pub success_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_closed_allows_calls() {
        let cb = CircuitBreaker::default();
        let result = cb.call("svc", || Ok::<_, String>(42));
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_opens_after_threshold_failures() {
        let config = BreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // 3 failures should open the circuit
        assert!(cb.call("svc", || Err::<(), _>("fail1")).is_err());
        assert!(cb.call("svc", || Err::<(), _>("fail2")).is_err());
        assert!(cb.call("svc", || Err::<(), _>("fail3")).is_err());

        assert_eq!(cb.state("svc"), CircuitState::Open);

        // Next call should be rejected immediately
        let result = cb.call("svc", || Ok::<_, String>("should not run"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("OPEN"));
    }

    #[test]
    fn test_reset_clears_state() {
        let config = BreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        assert!(cb.call("svc", || Err::<(), _>("fail")).is_err());
        assert_eq!(cb.state("svc"), CircuitState::Open);

        cb.reset("svc");
        assert_eq!(cb.state("svc"), CircuitState::Closed);
    }

    #[test]
    fn test_success_resets_failure_count_in_closed() {
        let config = BreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        assert!(cb.call("svc", || Err::<(), _>("f1")).is_err());
        assert!(cb.call("svc", || Err::<(), _>("f2")).is_err());
        // A success resets the count
        assert!(cb.call("svc", || Ok::<_, String>(())).is_ok());
        assert!(cb.call("svc", || Err::<(), _>("f3")).is_err());
        // Only 1 failure after reset, not 3, so circuit still closed
        assert_eq!(cb.state("svc"), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_transitions_to_closed_on_successes() {
        let config = BreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            timeout_secs: 0, // immediate half-open
            half_open_max_calls: 5,
        };
        let cb = CircuitBreaker::new(config);

        // Open the circuit
        assert!(cb.call("svc", || Err::<(), _>("f1")).is_err());
        assert!(cb.call("svc", || Err::<(), _>("f2")).is_err());
        assert_eq!(cb.state("svc"), CircuitState::Open);

        // With timeout=0, next check transitions to half-open
        assert!(cb.call("svc", || Ok::<_, String>("ok1")).is_ok());
        assert_eq!(cb.state("svc"), CircuitState::HalfOpen);

        // Continue succeeding
        assert!(cb.call("svc", || Ok::<_, String>("ok2")).is_ok());
        // success_threshold=2 → should be closed now
        assert_eq!(cb.state("svc"), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_failure_goes_back_to_open() {
        let config = BreakerConfig {
            failure_threshold: 1,
            success_threshold: 3,
            timeout_secs: 0,
            half_open_max_calls: 5,
        };
        let cb = CircuitBreaker::new(config);

        // Open the circuit
        assert!(cb.call("svc", || Err::<(), _>("f1")).is_err());
        assert_eq!(cb.state("svc"), CircuitState::Open);

        // Transition to half-open
        assert!(cb.call("svc", || Ok::<_, String>("ok")).is_ok());
        assert_eq!(cb.state("svc"), CircuitState::HalfOpen);

        // Failure in half-open → back to open
        assert!(cb.call("svc", || Err::<(), _>("fail_again")).is_err());
        assert_eq!(cb.state("svc"), CircuitState::Open);
    }

    #[test]
    fn test_half_open_limits_concurrent_calls() {
        let config = BreakerConfig {
            failure_threshold: 1,
            success_threshold: 5,
            timeout_secs: 0,
            half_open_max_calls: 2,
        };
        let cb = CircuitBreaker::new(config);

        // Open
        assert!(cb.call("svc", || Err::<(), _>("f1")).is_err());

        // Half-open: 2 calls allowed
        assert!(cb.call("svc", || Ok::<_, String>("ok1")).is_ok());
        assert!(cb.call("svc", || Ok::<_, String>("ok2")).is_ok());
        // 3rd call should be rejected
        let result = cb.call("svc", || Ok::<_, String>("too many"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("HALF_OPEN"));
    }

    #[test]
    fn test_reset_all() {
        let cb = CircuitBreaker::default();
        assert!(cb.call("a", || Err::<(), _>("f1")).is_err());
        assert!(cb.call("b", || Err::<(), _>("f2")).is_err());
        // failure_threshold=5, so still closed... force open by exhausting
        let cfg = BreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(cfg);
        assert!(cb.call("a", || Err::<(), _>("f1")).is_err());
        assert!(cb.call("b", || Err::<(), _>("f1")).is_err());
        assert_eq!(cb.state("a"), CircuitState::Open);
        assert_eq!(cb.state("b"), CircuitState::Open);

        cb.reset_all();
        assert_eq!(cb.state("a"), CircuitState::Closed);
        assert_eq!(cb.state("b"), CircuitState::Closed);
    }
}
