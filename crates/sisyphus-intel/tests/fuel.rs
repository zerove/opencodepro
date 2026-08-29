//! Integration tests for sisyphus-intel `fuel` pricing.
//!
//! Exercises the public `FuelPricer` trait, `SimpleFuelPricer`,
//! `AdaptiveFuelPricer`, default field values, and the
//! bounds-clamping behavior.

use sisyphus_intel::fuel::{AdaptiveFuelPricer, FuelPricer, SimpleFuelPricer};
use sisyphus_intel::SimulationResult;

fn sim(estimated_cost: u64) -> SimulationResult {
    SimulationResult {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost,
        predicted_outcome: "?".into(),
        predicted_effort: 0,
        predicted_stability: 1.0,
    }
}

// ---------------------------------------------------------------------------
// SimpleFuelPricer
// ---------------------------------------------------------------------------

#[test]
fn simple_pricer_doubles_current_fuel() {
    let p = SimpleFuelPricer;
    assert_eq!(p.price_fuel("n", 100, &sim(50)), 200);
    assert_eq!(p.price_fuel("n", 1000, &sim(9000)), 2000);
    assert_eq!(p.price_fuel("n", 0, &sim(0)), 0);
    assert_eq!(p.price_fuel("n", u64::MAX / 2, &sim(0)), u64::MAX - 1);
}

#[test]
fn simple_pricer_ignores_prediction_and_node_name() {
    let p = SimpleFuelPricer;
    // Two different predictions and two different node names, but same
    // current_fuel ⇒ same result.
    let a = p.price_fuel("alpha", 500, &sim(100));
    let b = p.price_fuel("beta", 500, &sim(9999));
    assert_eq!(a, b);
    assert_eq!(a, 1000);
}

// ---------------------------------------------------------------------------
// AdaptiveFuelPricer — defaults
// ---------------------------------------------------------------------------

#[test]
fn adaptive_pricer_default_fields() {
    let p = AdaptiveFuelPricer::default();
    assert!((p.headroom_factor - 1.5).abs() < 1e-9);
    assert!((p.shrink_factor - 0.8).abs() < 1e-9);
    assert_eq!(p.min_fuel, 100_000);
    assert_eq!(p.max_fuel, 10_000_000);
}

#[test]
fn adaptive_pricer_with_bounds_overrides_min_max() {
    let p = AdaptiveFuelPricer::new().with_bounds(50, 1000);
    assert_eq!(p.min_fuel, 50);
    assert_eq!(p.max_fuel, 1000);
    // headroom_factor / shrink_factor keep defaults.
    assert!((p.headroom_factor - 1.5).abs() < 1e-9);
    assert!((p.shrink_factor - 0.8).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// AdaptiveFuelPricer — three branches
// ---------------------------------------------------------------------------

#[test]
fn adaptive_increases_fuel_when_estimated_exceeds_current() {
    let p = AdaptiveFuelPricer::new().with_bounds(100, 1_000_000);
    let new_fuel = p.price_fuel("n", 1000, &sim(5000));
    // 5000 * 1.5 = 7500
    assert_eq!(new_fuel, 7500);
}

#[test]
fn adaptive_shrinks_fuel_when_estimated_is_below_half() {
    let p = AdaptiveFuelPricer::new().with_bounds(100, 1_000_000);
    let new_fuel = p.price_fuel("n", 10_000, &sim(2000));
    // 2000 < 10_000 / 2 = 5000 → shrink by 0.8
    assert_eq!(new_fuel, 8000);
}

#[test]
fn adaptive_keeps_fuel_when_in_band() {
    let p = AdaptiveFuelPricer::new().with_bounds(100, 1_000_000);
    // estimated = 5000, current = 10_000
    // 5000 < current_fuel (10_000) so the first branch (increase) is false
    // 5000 < current_fuel/2 (5000) — the comparison is `estimated < current_fuel/2`
    // In Rust integer division: 10_000/2 = 5000, so 5000 < 5000 is false.
    // Therefore the "shrink" branch is also false; we hit the keep branch.
    let new_fuel = p.price_fuel("n", 10_000, &sim(5_000));
    assert_eq!(new_fuel, 10_000);
}

// ---------------------------------------------------------------------------
// AdaptiveFuelPricer — bounds clamping
// ---------------------------------------------------------------------------

#[test]
fn adaptive_clamps_to_min_when_new_fuel_too_small() {
    let p = AdaptiveFuelPricer::new().with_bounds(500_000, 5_000_000);
    // estimated = 100, current = 100_000 → estimated < current/2 ⇒ shrink
    // new_fuel = 100_000 * 0.8 = 80_000 → clamped to 500_000
    let new_fuel = p.price_fuel("n", 100_000, &sim(100));
    assert_eq!(new_fuel, 500_000);
}

#[test]
fn adaptive_clamps_to_max_when_new_fuel_too_large() {
    let p = AdaptiveFuelPricer::new().with_bounds(100, 5_000_000);
    // estimated = 10_000_000 > current = 1_000_000 ⇒ increase
    // new_fuel = 10_000_000 * 1.5 = 15_000_000 → clamped to 5_000_000
    let new_fuel = p.price_fuel("n", 1_000_000, &sim(10_000_000));
    assert_eq!(new_fuel, 5_000_000);
}

#[test]
fn adaptive_clamps_when_shrink_yields_below_min() {
    let p = AdaptiveFuelPricer::new().with_bounds(2_000, 100_000);
    // estimated = 100 < current/2 ⇒ shrink
    // new_fuel = 2_000 * 0.8 = 1_600 → clamped to 2_000
    let new_fuel = p.price_fuel("n", 2_000, &sim(100));
    assert_eq!(new_fuel, 2_000);
}
