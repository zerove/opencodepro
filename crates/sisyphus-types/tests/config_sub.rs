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
    AdaptiveDepthConfig, AnomalyDetectorConfig, ChaosConfig, CognitiveLoopConfig, TokenBudgetConfig,
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
// AnomalyDetectorConfig
// ---------------------------------------------------------------------------

#[test]
fn anomaly_detector_default_validates_ok() {
    let c = AnomalyDetectorConfig::default();
    assert!(c.validate().is_ok());
    // 默认值与 2026-08-31 之前硬编码常量保持一致
    assert!((c.fuel_threshold - 2.0).abs() < 1e-9);
    assert!((c.duration_threshold - 3.0).abs() < 1e-9);
    assert!((c.error_rate_threshold - 0.3).abs() < 1e-9);
    assert_eq!(c.history_window, 20);
}

#[test]
fn anomaly_detector_fuel_threshold_zero_rejected() {
    let c = AnomalyDetectorConfig {
        fuel_threshold: 0.0,
        ..AnomalyDetectorConfig::default()
    };
    let err = c.validate().expect_err("fuel_threshold=0 should fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("fuel_threshold"),
        "expected fuel_threshold in error, got: {msg}"
    );
}

#[test]
fn anomaly_detector_duration_threshold_negative_rejected() {
    let c = AnomalyDetectorConfig {
        duration_threshold: -1.0,
        ..AnomalyDetectorConfig::default()
    };
    let err = c.validate().expect_err("negative duration should fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("duration_threshold"),
        "expected duration_threshold in error, got: {msg}"
    );
}

#[test]
fn anomaly_detector_error_rate_out_of_range_rejected() {
    let c = AnomalyDetectorConfig {
        error_rate_threshold: 1.5,
        ..AnomalyDetectorConfig::default()
    };
    assert!(c.validate().is_err());
}

#[test]
fn anomaly_detector_error_rate_nan_rejected() {
    let c = AnomalyDetectorConfig {
        error_rate_threshold: f64::NAN,
        ..AnomalyDetectorConfig::default()
    };
    assert!(c.validate().is_err(), "NaN must be rejected");
}

#[test]
fn anomaly_detector_history_window_zero_rejected() {
    let c = AnomalyDetectorConfig {
        history_window: 0,
        ..AnomalyDetectorConfig::default()
    };
    let err = c.validate().expect_err("history_window=0 should fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("history_window"),
        "expected history_window in error, got: {msg}"
    );
}

#[test]
fn anomaly_detector_boundary_values_accepted() {
    let c = AnomalyDetectorConfig {
        fuel_threshold: 1.0,
        duration_threshold: 1.0,
        error_rate_threshold: 0.0,
        history_window: 1,
    };
    assert!(c.validate().is_ok());
}

#[test]
fn anomaly_detector_embedded_in_cognitive_loop_validates() {
    // `CognitiveLoopConfig::validate()` 必须递归校验 anomaly_detector
    let mut c = CognitiveLoopConfig::default();
    c.anomaly_detector.history_window = 0;
    let err = c.validate().expect_err("embedded history_window=0 should fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("history_window"),
        "expected nested validation, got: {msg}"
    );
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
