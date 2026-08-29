//! Integration tests for sisyphus-intel `CognitiveDepth`.
//!
//! Exercises the pure enum-to-config mapping and `should_simulate` so we
//! catch regressions if new depth tiers are added. The async
//! `ComplexityAnalyzer::analyze` is exercised separately by internal
//! unit tests (requires `ExperienceBank` mock).

use sisyphus_intel::CognitiveDepth;

#[test]
fn depth_ordering_matches_documented_order() {
    // CognitiveDepth derives Ord, so verify the explicit variant order.
    assert!(CognitiveDepth::Low < CognitiveDepth::Medium);
    assert!(CognitiveDepth::Medium < CognitiveDepth::High);
    assert!(CognitiveDepth::High < CognitiveDepth::Ultra);
}

#[test]
fn to_config_yields_strictly_increasing_iteration_budgets() {
    let low = CognitiveDepth::Low.to_config();
    let med = CognitiveDepth::Medium.to_config();
    let high = CognitiveDepth::High.to_config();
    let ultra = CognitiveDepth::Ultra.to_config();

    // Iteration budgets (first element) must be strictly increasing
    // so that deeper analyses do more work.
    assert!(low.0 < med.0, "Medium iterations ({}) must exceed Low ({})", med.0, low.0);
    assert!(med.0 < high.0);
    assert!(high.0 < ultra.0);

    // Thresholds (second element) must be strictly decreasing
    // so that deeper analyses are more selective.
    assert!(low.1 > med.1);
    assert!(med.1 > high.1);
    assert!(high.1 > ultra.1);
}

#[test]
fn only_low_skips_simulation() {
    // Contract: simulation is skipped for trivially small problems only.
    assert!(!CognitiveDepth::Low.should_simulate());
    assert!(CognitiveDepth::Medium.should_simulate());
    assert!(CognitiveDepth::High.should_simulate());
    assert!(CognitiveDepth::Ultra.should_simulate());
}

#[test]
fn to_config_thresholds_are_in_unit_interval() {
    // All documented thresholds are probabilities; none should exceed 1.0.
    for depth in [
        CognitiveDepth::Low,
        CognitiveDepth::Medium,
        CognitiveDepth::High,
        CognitiveDepth::Ultra,
    ] {
        let (iters, threshold) = depth.to_config();
        assert!(iters > 0, "{depth:?} should have a positive iteration budget");
        assert!(
            (0.0..=1.0).contains(&threshold),
            "{depth:?} threshold {threshold} must be in [0.0, 1.0]"
        );
    }
}

// ---------------------------------------------------------------------------
// Additional: per-variant fields + should_simulate + Copy/Debug
// ---------------------------------------------------------------------------

#[test]
fn cd_cognitive_depth_variants_constructible() {
    let _ = CognitiveDepth::Low;
    let _ = CognitiveDepth::Medium;
    let _ = CognitiveDepth::High;
    let _ = CognitiveDepth::Ultra;
}

#[test]
fn cd_cognitive_depth_equality() {
    assert_eq!(CognitiveDepth::Low, CognitiveDepth::Low);
    assert_eq!(CognitiveDepth::Ultra, CognitiveDepth::Ultra);
    assert_ne!(CognitiveDepth::Low, CognitiveDepth::Medium);
    assert_ne!(CognitiveDepth::High, CognitiveDepth::Ultra);
}

#[test]
fn cd_cognitive_depth_copy() {
    let a = CognitiveDepth::Medium;
    let _b = a;
    let _c = a;
}

#[test]
fn cd_cognitive_depth_to_config_low() {
    let (max_iter, threshold) = CognitiveDepth::Low.to_config();
    assert_eq!(max_iter, 3);
    assert!((threshold - 0.2).abs() < 1e-9);
}

#[test]
fn cd_cognitive_depth_to_config_medium() {
    let (max_iter, threshold) = CognitiveDepth::Medium.to_config();
    assert_eq!(max_iter, 10);
    assert!((threshold - 0.05).abs() < 1e-9);
}

#[test]
fn cd_cognitive_depth_to_config_high() {
    let (max_iter, threshold) = CognitiveDepth::High.to_config();
    assert_eq!(max_iter, 20);
    assert!((threshold - 0.01).abs() < 1e-9);
}

#[test]
fn cd_cognitive_depth_to_config_ultra() {
    let (max_iter, threshold) = CognitiveDepth::Ultra.to_config();
    assert_eq!(max_iter, 30);
    assert!((threshold - 0.005).abs() < 1e-9);
}

#[test]
fn cd_should_simulate_low_false() {
    assert!(!CognitiveDepth::Low.should_simulate());
}

#[test]
fn cd_should_simulate_medium_true() {
    assert!(CognitiveDepth::Medium.should_simulate());
}

#[test]
fn cd_should_simulate_high_true() {
    assert!(CognitiveDepth::High.should_simulate());
}

#[test]
fn cd_should_simulate_ultra_true() {
    assert!(CognitiveDepth::Ultra.should_simulate());
}

#[test]
fn cd_cognitive_depth_debug() {
    assert_eq!(format!("{:?}", CognitiveDepth::Low), "Low");
    assert_eq!(format!("{:?}", CognitiveDepth::Ultra), "Ultra");
}
