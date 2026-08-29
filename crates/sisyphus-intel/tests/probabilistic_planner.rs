//! Integration tests for sisyphus-intel `ProbabilisticPlanner`.
//!
//! Exercises the public `ProbabilisticPlanner` / `RankedPath` / `RiskLevel` /
//! `PlannerConfig` / `RiskThresholds` / `PathSummary` / `ExperienceRecord` API.
//!
//! The planner consumes `SimulationResult`s and produces ranked candidate
//! execution paths. It composes success probability, cost, and stability into
//! a composite score, classifies each path by risk, and uses historical
//! experience records to bias probabilities.

use sisyphus_intel::probabilistic_planner::{
    ExperienceRecord, PlannerConfig, ProbabilisticPlanner, RiskLevel, RiskThresholds,
};
use sisyphus_intel::SimulationResult;

fn sim(path_id: &str, prob: f64, cost: u64, stability: f64, outcome: &str) -> SimulationResult {
    SimulationResult {
        path_id: path_id.to_string(),
        success_probability: prob,
        estimated_cost: cost,
        predicted_stability: stability,
        predicted_effort: cost,
        predicted_outcome: outcome.to_string(),
    }
}

// ---------------------------------------------------------------------------
// RiskLevel ordering
// ---------------------------------------------------------------------------

#[test]
fn risk_level_ord_matches_intent() {
    assert!(RiskLevel::Low < RiskLevel::Medium);
    assert!(RiskLevel::Medium < RiskLevel::High);
    assert!(RiskLevel::High < RiskLevel::Critical);
}

// ---------------------------------------------------------------------------
// RiskThresholds default
// ---------------------------------------------------------------------------

#[test]
fn risk_thresholds_default_values() {
    let t = RiskThresholds::default();
    assert_eq!(t.low, 0.8);
    assert_eq!(t.medium, 0.6);
    assert_eq!(t.high, 0.4);
}

#[test]
fn planner_config_default_is_sensible() {
    let c = PlannerConfig::default();
    assert_eq!(c.probability_weight, 0.5);
    assert_eq!(c.cost_weight, 0.3);
    assert_eq!(c.stability_weight, 0.2);
    assert_eq!(c.max_cost, 10_000);
    // Weights don't necessarily sum to 1.0, but should be non-negative.
    assert!(c.probability_weight >= 0.0);
    assert!(c.cost_weight >= 0.0);
    assert!(c.stability_weight >= 0.0);
}

// ---------------------------------------------------------------------------
// compute_risk_score
// ---------------------------------------------------------------------------

#[test]
fn compute_risk_score_clamps_to_unit_interval() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());

    // Perfect case: prob=1, cost=0, stability=1 → risk = 0
    let r = sim("a", 1.0, 0, 1.0, "");
    assert!(p.compute_risk_score(&r) < 1e-9);

    // Worst case: prob=0, cost>>max, stability=0 → risk clamped to 1.0
    let r = sim("b", 0.0, 100_000, 0.0, "");
    let risk = p.compute_risk_score(&r);
    assert!((risk - 1.0).abs() < 1e-9, "got {risk}");

    // Mid case
    let r = sim("c", 0.5, 5_000, 0.5, "");
    let risk = p.compute_risk_score(&r);
    assert!(risk > 0.0 && risk < 1.0, "got {risk}");
}

#[test]
fn compute_risk_score_higher_cost_increases_risk() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    let cheap = sim("a", 0.9, 100, 0.9, "");
    let expensive = sim("a", 0.9, 9_900, 0.9, "");
    assert!(p.compute_risk_score(&expensive) > p.compute_risk_score(&cheap));
}

// ---------------------------------------------------------------------------
// classify_risk
// ---------------------------------------------------------------------------

#[test]
fn classify_risk_thresholds() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    // Default thresholds: low=0.8, medium=0.6, high=0.4
    // Implementation: score <= (1.0 - t.low) etc.
    // Note: f64 imprecision means 1.0 - 0.8 = 0.19999... (not exactly 0.2)
    //   score 0.19  → Low (< 0.19999...)
    //   score 0.21  → Medium
    //   score 0.39  → Medium (< 1.0 - 0.6 = 0.4)
    //   score 0.41  → High
    //   score 0.59  → High (< 1.0 - 0.4 = 0.6)
    //   score 0.61  → Critical
    assert_eq!(p.classify_risk(0.0), RiskLevel::Low);
    assert_eq!(p.classify_risk(0.19), RiskLevel::Low);
    assert_eq!(p.classify_risk(0.21), RiskLevel::Medium);  // 0.2 would round to 0.19999... due to f64
    assert_eq!(p.classify_risk(0.39), RiskLevel::Medium);
    assert_eq!(p.classify_risk(0.41), RiskLevel::High);
    assert_eq!(p.classify_risk(0.59), RiskLevel::High);
    assert_eq!(p.classify_risk(0.61), RiskLevel::Critical);
    assert_eq!(p.classify_risk(1.0), RiskLevel::Critical);
}

// ---------------------------------------------------------------------------
// adjust_probability_with_experience
// ---------------------------------------------------------------------------

#[test]
fn adjust_probability_no_experience_returns_base() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    let adj = p.adjust_probability_with_experience("p1", 0.7);
    assert!((adj - 0.7).abs() < 1e-9);
}

#[test]
fn adjust_probability_blends_60_40_with_history() {
    let exp = vec![
        // 2 success, 1 fail → historical_rate = 2/3 ≈ 0.6667
        ExperienceRecord {
            path_id: "p1".into(),
            success: true,
            actual_cost: 100,
        },
        ExperienceRecord {
            path_id: "p1".into(),
            success: true,
            actual_cost: 110,
        },
        ExperienceRecord {
            path_id: "p1".into(),
            success: false,
            actual_cost: 200,
        },
    ];
    let p = ProbabilisticPlanner::with_config_and_experiences(PlannerConfig::default(), exp);

    let base = 0.5;
    let adj = p.adjust_probability_with_experience("p1", base);
    // 0.5 * 0.6 + (2/3) * 0.4 = 0.3 + 0.2667 = 0.5667
    let expected = 0.5 * 0.6 + (2.0 / 3.0) * 0.4;
    assert!((adj - expected).abs() < 1e-6, "got {adj}, expected {expected}");
}

#[test]
fn adjust_probability_ignores_other_paths() {
    let exp = vec![
        ExperienceRecord {
            path_id: "p2".into(),
            success: true,
            actual_cost: 0,
        };
        10
    ];
    let p = ProbabilisticPlanner::with_config_and_experiences(PlannerConfig::default(), exp);
    // No experience for p1 → base returned unchanged.
    let adj = p.adjust_probability_with_experience("p1", 0.42);
    assert!((adj - 0.42).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// rank_paths
// ---------------------------------------------------------------------------

#[test]
fn rank_paths_empty_input_returns_empty() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    assert!(p.rank_paths(&[]).is_empty());
}

#[test]
fn rank_paths_orders_by_composite_score_desc() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    let paths = vec![
        sim("worst", 0.3, 9000, 0.2, ""),
        sim("best", 0.95, 100, 0.95, ""),
        sim("mid", 0.7, 1000, 0.7, ""),
    ];
    let ranked = p.rank_paths(&paths);
    assert_eq!(ranked[0].path_id, "best");
    assert_eq!(ranked[2].path_id, "worst");
    // composite scores must be monotonically non-increasing
    assert!(ranked[0].composite_score >= ranked[1].composite_score);
    assert!(ranked[1].composite_score >= ranked[2].composite_score);
}

#[test]
fn rank_paths_preserves_input_ids() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    let paths = vec![sim("alpha", 0.5, 100, 0.5, ""), sim("beta", 0.6, 100, 0.5, "")];
    let ranked = p.rank_paths(&paths);
    let ids: Vec<&str> = ranked.iter().map(|r| r.path_id.as_str()).collect();
    assert!(ids.contains(&"alpha"));
    assert!(ids.contains(&"beta"));
}

// ---------------------------------------------------------------------------
// select_optimal_path
// ---------------------------------------------------------------------------

#[test]
fn select_optimal_path_empty_returns_none() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    assert!(p.select_optimal_path(&[]).is_none());
}

#[test]
fn select_optimal_path_prefers_lower_risk_when_scores_similar() {
    // Default config: prob=0.5/cost=0.3/stability=0.2.
    // A high-risk, high-probability path may outscore a low-risk one
    // if composite math favors prob heavily. We craft a case where the
    // best composite is also the safest by stacking stability/prob for it.
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    let paths = vec![
        // A: medium everything
        sim("A", 0.7, 1000, 0.7, ""),
        // B: high cost, low stability — likely Critical
        sim("B", 0.9, 9_900, 0.1, ""),
    ];
    let chosen = p.select_optimal_path(&paths).expect("some path");
    // A is the safer of the two and the test should pick it (B is Critical
    // risk and gets filtered).
    assert_eq!(chosen.path_id, "A");
}

#[test]
fn select_optimal_path_falls_back_when_all_critical() {
    // Force all paths to be Critical risk by giving them prob=0/cost=max.
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    let paths = vec![
        sim("x", 0.0, 10_000, 0.0, ""),
        sim("y", 0.0, 10_000, 0.0, ""),
    ];
    // Both Critical; select_optimal_path returns the best (highest composite)
    // even if all are Critical.
    let chosen = p.select_optimal_path(&paths).expect("must pick one");
    assert!(chosen.path_id == "x" || chosen.path_id == "y");
}

// ---------------------------------------------------------------------------
// summarize
// ---------------------------------------------------------------------------

#[test]
fn summarize_empty_returns_default() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    let s = p.summarize(&[]);
    assert_eq!(s.total_paths, 0);
    assert_eq!(s.average_probability, 0.0);
    assert_eq!(s.average_cost, 0);
    assert!(s.best_path.is_none());
    assert!(s.worst_path.is_none());
    assert!(s.risk_distribution.is_empty());
}

#[test]
fn summarize_aggregates_averages_and_risk_distribution() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    let paths = vec![
        sim("a", 0.5, 1_000, 0.5, ""),
        sim("b", 0.7, 2_000, 0.7, ""),
        sim("c", 0.9, 3_000, 0.9, ""),
    ];
    let s = p.summarize(&paths);
    assert_eq!(s.total_paths, 3);
    // avg prob: (0.5 + 0.7 + 0.9)/3 = 0.7
    assert!((s.average_probability - 0.7).abs() < 1e-6);
    // avg cost: (1000+2000+3000)/3 = 2000
    assert_eq!(s.average_cost, 2_000);
    assert!(s.best_path.is_some());
    assert!(s.worst_path.is_some());
    // risk_distribution should sum to 3.
    let total: usize = s.risk_distribution.values().sum();
    assert_eq!(total, 3);
}

// ---------------------------------------------------------------------------
// RankedPath fields
// ---------------------------------------------------------------------------

#[test]
fn ranked_path_predicted_outcome_preserved() {
    let p = ProbabilisticPlanner::new(PlannerConfig::default());
    let paths = vec![sim("p", 0.8, 100, 0.8, "success")];
    let ranked = p.rank_paths(&paths);
    assert_eq!(ranked[0].predicted_outcome, "success");
    assert!((ranked[0].success_probability - 0.8).abs() < 1e-9);
    assert_eq!(ranked[0].estimated_cost, 100);
}
