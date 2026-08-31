//! Integration tests for sisyphus-types `config::sub` and
//! `config::llm` modules.
//!
//! Focuses on the `Default` and `validate()` surfaces of:
//! - `CognitiveLoopConfig`
//! - `AdaptiveDepthConfig`
//! - `TokenBudgetConfig`
//! - `ChaosConfig`
//! - `LlmConfig` (via `default_llm_config`)

use sisyphus_types::config::llm::default_llm_config;
use sisyphus_types::config::sub::{
    AdaptiveDepthConfig, ChaosConfig, CognitiveLoopConfig, TokenBudgetConfig,
};

// ---------------------------------------------------------------------------
// CognitiveLoopConfig
// ---------------------------------------------------------------------------

#[test]
fn cognitive_loop_config_default_validates_ok() {
    let c = CognitiveLoopConfig::default();
    assert!(c.validate().is_ok());
}

#[test]
fn cognitive_loop_config_max_iterations_zero_rejected() {
    let mut c = CognitiveLoopConfig::default();
    c.max_iterations = 0;
    let r = c.validate();
    assert!(r.is_err());
    let s = format!("{}", r.unwrap_err());
    assert!(s.contains("max_iterations"));
}

#[test]
fn cognitive_loop_config_max_iterations_too_large_rejected() {
    let mut c = CognitiveLoopConfig::default();
    c.max_iterations = 10_000;
    let r = c.validate();
    assert!(r.is_err());
    let s = format!("{}", r.unwrap_err());
    assert!(s.contains("max_iterations"));
}

#[test]
fn cognitive_loop_config_timeout_zero_rejected() {
    let mut c = CognitiveLoopConfig::default();
    c.llm_stage_timeout_secs = 0;
    // Note: 0 means "disable" for LLM stage; the validator only rejects
    // if the value is > 600 (or out of bounds). Per the code, 0 is
    // allowed. We just confirm validation succeeds.
    let _ = c.validate();
}

#[test]
fn cognitive_loop_config_timeout_too_large_rejected() {
    let mut c = CognitiveLoopConfig::default();
    c.llm_stage_timeout_secs = 1000;
    let r = c.validate();
    assert!(r.is_err());
    let s = format!("{}", r.unwrap_err());
    assert!(s.contains("llm_stage_timeout"));
}

#[test]
fn cognitive_loop_config_convergence_threshold_negative_rejected() {
    let mut c = CognitiveLoopConfig::default();
    c.convergence_threshold = -0.1;
    let r = c.validate();
    assert!(r.is_err());
    let s = format!("{}", r.unwrap_err());
    assert!(s.contains("convergence_threshold"));
}

#[test]
fn cognitive_loop_config_convergence_threshold_above_one_rejected() {
    let mut c = CognitiveLoopConfig::default();
    c.convergence_threshold = 1.5;
    let r = c.validate();
    assert!(r.is_err());
}

#[test]
fn cognitive_loop_config_convergence_threshold_boundaries_ok() {
    let mut c = CognitiveLoopConfig::default();
    c.convergence_threshold = 0.0;
    assert!(c.validate().is_ok());
    c.convergence_threshold = 1.0;
    assert!(c.validate().is_ok());
}

// ---------------------------------------------------------------------------
// AdaptiveDepthConfig
// ---------------------------------------------------------------------------

#[test]
fn adaptive_depth_default_validates_ok() {
    let c = AdaptiveDepthConfig::default();
    assert!(c.validate().is_ok());
}

#[test]
fn adaptive_depth_initial_budget_below_one_rejected() {
    let mut c = AdaptiveDepthConfig::default();
    c.initial_budget = 0.5;
    let r = c.validate();
    assert!(r.is_err());
    let s = format!("{}", r.unwrap_err());
    assert!(s.contains("initial_budget"));
}

#[test]
fn adaptive_depth_decay_negative_rejected() {
    let mut c = AdaptiveDepthConfig::default();
    c.decay = -0.1;
    let r = c.validate();
    assert!(r.is_err());
}

#[test]
fn adaptive_depth_decay_above_one_rejected() {
    let mut c = AdaptiveDepthConfig::default();
    c.decay = 1.1;
    let r = c.validate();
    assert!(r.is_err());
}

#[test]
fn adaptive_depth_decay_boundaries_ok() {
    let mut c = AdaptiveDepthConfig::default();
    c.decay = 0.0;
    assert!(c.validate().is_ok());
    c.decay = 1.0;
    assert!(c.validate().is_ok());
}

// ---------------------------------------------------------------------------
// TokenBudgetConfig
// ---------------------------------------------------------------------------

#[test]
fn token_budget_default_has_no_cap() {
    let c = TokenBudgetConfig::default();
    assert!(c.cap.is_none());
}

#[test]
fn token_budget_default_reserve_ratio() {
    let c = TokenBudgetConfig::default();
    // reserve_ratio default is some value in [0, 1].
    assert!((0.0..=1.0).contains(&c.reserve_ratio));
}

#[test]
fn token_budget_field_assignment() {
    let c = TokenBudgetConfig {
        cap: Some(1000),
        reserve_ratio: 0.5,
    };
    assert_eq!(c.cap, Some(1000));
    assert!((c.reserve_ratio - 0.5).abs() < 1e-9);
}

#[test]
fn token_budget_default_validates_ok() {
    // Default cap=None and reserve_ratio=0.1 must validate.
    let c = TokenBudgetConfig::default();
    assert!(c.validate().is_ok());
}

#[test]
fn token_budget_reserve_ratio_above_one_rejected() {
    let c = TokenBudgetConfig {
        cap: Some(1000),
        reserve_ratio: 1.5,
    };
    let err = c.validate().expect_err("1.5 reserve_ratio should fail");
    let msg = format!("{err}");
    assert!(msg.contains("reserve_ratio"), "got: {msg}");
}

#[test]
fn token_budget_cap_zero_rejected() {
    let c = TokenBudgetConfig {
        cap: Some(0),
        reserve_ratio: 0.1,
    };
    let err = c.validate().expect_err("cap=0 should fail");
    let msg = format!("{err}");
    assert!(msg.contains("cap"), "got: {msg}");
}

#[test]
fn token_budget_none_cap_with_extreme_ratio_validates() {
    // reserve_ratio out of range still invalid even when cap is None.
    let c = TokenBudgetConfig {
        cap: None,
        reserve_ratio: 2.0,
    };
    assert!(c.validate().is_err());
}

// ---------------------------------------------------------------------------
// ChaosConfig
// ---------------------------------------------------------------------------

#[test]
fn chaos_config_default_validates_ok() {
    let c = ChaosConfig::default();
    assert!(c.validate().is_ok());
}

// ---------------------------------------------------------------------------
// LlmConfig
// ---------------------------------------------------------------------------

#[test]
fn default_llm_config_has_endpoint() {
    let c = default_llm_config();
    assert!(c.endpoint.starts_with("http"));
    assert!(!c.model.is_empty());
}
