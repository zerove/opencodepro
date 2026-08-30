//! Integration tests for sisyphus-intel `complexity`.
//!
//! Exercises the public [`CognitiveDepth`] API — the discrete depth
//! classification used by the cognitive loop. Each depth level
//! maps to a (max_iterations, convergence_threshold) tuple via
//! `to_config`, and `should_simulate` decides whether the
//! simulation phase is skipped for trivial tasks.

use sisyphus_intel::complexity::CognitiveDepth;

// ---------------------------------------------------------------------------
// to_config
// ---------------------------------------------------------------------------

#[test]
fn low_depth_to_config() {
    let (max_iter, threshold) = CognitiveDepth::Low.to_config();
    assert_eq!(max_iter, 3);
    assert!((threshold - 0.2).abs() < 1e-9);
}

#[test]
fn medium_depth_to_config() {
    let (max_iter, threshold) = CognitiveDepth::Medium.to_config();
    assert_eq!(max_iter, 10);
    assert!((threshold - 0.05).abs() < 1e-9);
}

#[test]
fn high_depth_to_config() {
    let (max_iter, threshold) = CognitiveDepth::High.to_config();
    assert_eq!(max_iter, 20);
    assert!((threshold - 0.01).abs() < 1e-9);
}

#[test]
fn ultra_depth_to_config() {
    let (max_iter, threshold) = CognitiveDepth::Ultra.to_config();
    assert_eq!(max_iter, 30);
    assert!((threshold - 0.005).abs() < 1e-9);
}

#[test]
fn depth_max_iterations_increases_monotonically() {
    let low = CognitiveDepth::Low.to_config().0;
    let med = CognitiveDepth::Medium.to_config().0;
    let high = CognitiveDepth::High.to_config().0;
    let ultra = CognitiveDepth::Ultra.to_config().0;
    assert!(low < med, "low {low} < med {med}");
    assert!(med < high, "med {med} < high {high}");
    assert!(high < ultra, "high {high} < ultra {ultra}");
}

#[test]
fn depth_threshold_decreases_monotonically() {
    let low = CognitiveDepth::Low.to_config().1;
    let med = CognitiveDepth::Medium.to_config().1;
    let high = CognitiveDepth::High.to_config().1;
    let ultra = CognitiveDepth::Ultra.to_config().1;
    assert!(low > med);
    assert!(med > high);
    assert!(high > ultra);
}

// ---------------------------------------------------------------------------
// should_simulate
// ---------------------------------------------------------------------------

#[test]
fn low_depth_skips_simulation() {
    assert!(!CognitiveDepth::Low.should_simulate());
}

#[test]
fn medium_depth_simulates() {
    assert!(CognitiveDepth::Medium.should_simulate());
}

#[test]
fn high_depth_simulates() {
    assert!(CognitiveDepth::High.should_simulate());
}

#[test]
fn ultra_depth_simulates() {
    assert!(CognitiveDepth::Ultra.should_simulate());
}

// ---------------------------------------------------------------------------
// Comparison / ordering
// ---------------------------------------------------------------------------

#[test]
fn depth_ordering_low_lt_medium() {
    assert!(CognitiveDepth::Low < CognitiveDepth::Medium);
}

#[test]
fn depth_ordering_medium_lt_high() {
    assert!(CognitiveDepth::Medium < CognitiveDepth::High);
}

#[test]
fn depth_ordering_high_lt_ultra() {
    assert!(CognitiveDepth::High < CognitiveDepth::Ultra);
}

#[test]
fn depth_equality() {
    assert_eq!(CognitiveDepth::Low, CognitiveDepth::Low);
    assert_ne!(CognitiveDepth::Low, CognitiveDepth::Medium);
}

#[test]
fn depth_clone_and_copy() {
    let a = CognitiveDepth::High;
    let b = a; // Copy
    let c = a; // Used after copy → Copy is fine
    let d = a.clone(); // Clone also works
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert_eq!(a, d);
}

#[test]
fn all_distinct_depths_have_distinct_configs() {
    let low = CognitiveDepth::Low.to_config();
    let med = CognitiveDepth::Medium.to_config();
    let high = CognitiveDepth::High.to_config();
    let ultra = CognitiveDepth::Ultra.to_config();
    assert_ne!(low, med);
    assert_ne!(med, high);
    assert_ne!(high, ultra);
    assert_ne!(low, high);
    assert_ne!(low, ultra);
    assert_ne!(med, ultra);
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn depth_debug_format() {
    let _ = format!("{:?}", CognitiveDepth::Low);
    let _ = format!("{:?}", CognitiveDepth::Medium);
    let _ = format!("{:?}", CognitiveDepth::High);
    let _ = format!("{:?}", CognitiveDepth::Ultra);
}

#[test]
fn depth_should_simulate_consistent() {
    // should_simulate is a function of the depth, not the configuration
    let depths = [CognitiveDepth::Low, CognitiveDepth::Medium, CognitiveDepth::High, CognitiveDepth::Ultra];
    for d in depths {
        let _ = d.should_simulate();
    }
}

#[test]
fn depth_to_config_returns_positive_thresholds() {
    for d in [CognitiveDepth::Low, CognitiveDepth::Medium, CognitiveDepth::High, CognitiveDepth::Ultra] {
        let (max_iter, threshold) = d.to_config();
        assert!(max_iter > 0);
        assert!(threshold > 0.0);
        assert!(threshold < 1.0);
    }
}

#[test]
fn depth_thresholds_strictly_decreasing() {
    let (_, t_low) = CognitiveDepth::Low.to_config();
    let (_, t_med) = CognitiveDepth::Medium.to_config();
    let (_, t_high) = CognitiveDepth::High.to_config();
    let (_, t_ultra) = CognitiveDepth::Ultra.to_config();
    assert!(t_low > t_med);
    assert!(t_med > t_high);
    assert!(t_high > t_ultra);
}

#[test]
fn depth_iterations_strictly_increasing() {
    let (i_low, _) = CognitiveDepth::Low.to_config();
    let (i_med, _) = CognitiveDepth::Medium.to_config();
    let (i_high, _) = CognitiveDepth::High.to_config();
    let (i_ultra, _) = CognitiveDepth::Ultra.to_config();
    assert!(i_low < i_med);
    assert!(i_med < i_high);
    assert!(i_high < i_ultra);
}
