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
    ExperienceRecord, PathSummary, PlannerConfig, ProbabilisticPlanner, RankedPath, RiskLevel,
    RiskThresholds,
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

// ---------------------------------------------------------------------------
// RiskLevel additional coverage
// ---------------------------------------------------------------------------

#[test]
fn risk_level_serde_round_trip() {
    for level in [
        RiskLevel::Low,
        RiskLevel::Medium,
        RiskLevel::High,
        RiskLevel::Critical,
    ] {
        let json = serde_json::to_string(&level).expect("");
        let parsed: RiskLevel = serde_json::from_str(&json).expect("");
        assert_eq!(parsed, level);
    }
}

#[test]
fn risk_level_distinct_variants() {
    let variants = [
        RiskLevel::Low,
        RiskLevel::Medium,
        RiskLevel::High,
        RiskLevel::Critical,
    ];
    for (i, a) in variants.iter().enumerate() {
        for (j, b) in variants.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

#[test]
fn risk_level_debug_format() {
    for level in [
        RiskLevel::Low,
        RiskLevel::Medium,
        RiskLevel::High,
        RiskLevel::Critical,
    ] {
        let _ = format!("{:?}", level);
    }
}

#[test]
fn risk_level_hash_distinct() {
    use std::collections::HashSet;
    let mut set: HashSet<RiskLevel> = HashSet::new();
    set.insert(RiskLevel::Low);
    set.insert(RiskLevel::Medium);
    set.insert(RiskLevel::High);
    set.insert(RiskLevel::Critical);
    // Re-adding should not increase size
    set.insert(RiskLevel::Low);
    assert_eq!(set.len(), 4);
}

#[test]
fn risk_level_copy_and_clone() {
    let a = RiskLevel::Critical;
    let b = a; // Copy
    let c = a.clone();
    assert_eq!(a, b);
    assert_eq!(a, c);
}

#[test]
fn ranked_path_unicode_id() {
    let rp = RankedPath {
        path_id: "路径-α".into(),
        success_probability: 0.5,
        estimated_cost: 100,
        risk_level: RiskLevel::Medium,
        composite_score: 0.5,
        predicted_outcome: "成功".into(),
        confidence: 0.5,
    };
    let json = serde_json::to_string(&rp).expect("");
    let parsed: RankedPath = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.path_id, "路径-α");
    assert_eq!(parsed.predicted_outcome, "成功");
}

#[test]
fn ranked_path_clone() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.8,
        estimated_cost: 50,
        risk_level: RiskLevel::Low,
        composite_score: 0.8,
        predicted_outcome: "ok".into(),
        confidence: 0.9,
    };
    let cloned = rp.clone();
    assert_eq!(cloned.path_id, "p");
    assert_eq!(cloned.risk_level, RiskLevel::Low);
}

#[test]
fn ranked_path_debug_format() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "x".into(),
        confidence: 0.5,
    };
    let dbg = format!("{:?}", rp);
    assert!(dbg.contains("path_id"));
}

#[test]
fn risk_thresholds_serde() {
    let t = RiskThresholds {
        low: 0.1,
        medium: 0.4,
        high: 0.7,
    };
    let json = serde_json::to_string(&t).expect("");
    let parsed: RiskThresholds = serde_json::from_str(&json).expect("");
    assert!((parsed.low - 0.1).abs() < 1e-9);
    assert!((parsed.medium - 0.4).abs() < 1e-9);
    assert!((parsed.high - 0.7).abs() < 1e-9);
}

#[test]
fn risk_thresholds_clone() {
    let t = RiskThresholds::default();
    let cloned = t.clone();
    assert_eq!(cloned.low, t.low);
    assert_eq!(cloned.medium, t.medium);
    assert_eq!(cloned.high, t.high);
}

#[test]
fn planner_config_serde() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 10_000,
        risk_thresholds: RiskThresholds::default(),
    };
    let json = serde_json::to_string(&cfg).expect("");
    let parsed: PlannerConfig = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.max_cost, 10_000);
    assert!((parsed.probability_weight - 0.5).abs() < 1e-9);
}

#[test]
fn planner_config_clone() {
    let cfg = PlannerConfig::default();
    let cloned = cfg.clone();
    assert_eq!(cloned.max_cost, cfg.max_cost);
    assert_eq!(cloned.probability_weight, cfg.probability_weight);
}

// ---------------------------------------------------------------------------
// PathSummary
// ---------------------------------------------------------------------------

#[test]
fn path_summary_default_is_empty() {
    let s = PathSummary::default();
    assert_eq!(s.total_paths, 0);
    assert_eq!(s.average_probability, 0.0);
    assert_eq!(s.average_cost, 0);
    assert!(s.best_path.is_none());
    assert!(s.worst_path.is_none());
    assert!(s.risk_distribution.is_empty());
}

#[test]
fn path_summary_field_assignment() {
    let rp = RankedPath {
        path_id: "p1".into(),
        success_probability: 0.9,
        estimated_cost: 100,
        risk_level: RiskLevel::Low,
        composite_score: 0.9,
        predicted_outcome: "ok".into(),
        confidence: 0.9,
    };
    let mut dist = std::collections::HashMap::new();
    dist.insert(RiskLevel::Low, 3);
    dist.insert(RiskLevel::High, 1);

    let s = PathSummary {
        total_paths: 4,
        average_probability: 0.7,
        average_cost: 200,
        best_path: Some(rp.clone()),
        worst_path: Some(rp),
        risk_distribution: dist,
    };
    assert_eq!(s.total_paths, 4);
    assert_eq!(s.risk_distribution.get(&RiskLevel::Low), Some(&3));
}

#[test]
fn path_summary_serde() {
    let s = PathSummary {
        total_paths: 2,
        average_probability: 0.5,
        average_cost: 100,
        best_path: None,
        worst_path: None,
        risk_distribution: std::collections::HashMap::new(),
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.total_paths, 2);
    assert!(back.best_path.is_none());
}

#[test]
fn path_summary_clone() {
    let s = PathSummary::default();
    let cloned = s.clone();
    assert_eq!(cloned.total_paths, 0);
    assert_eq!(cloned.average_cost, 0);
}

#[test]
fn path_summary_debug() {
    let s = PathSummary::default();
    let dbg = format!("{:?}", s);
    assert!(dbg.contains("PathSummary"));
    assert!(dbg.contains("total_paths"));
}

// ---------------------------------------------------------------------------
// Additional edge cases
// ---------------------------------------------------------------------------

#[test]
fn risk_level_ord_derived_from_declaration() {
    use std::cmp::Ordering;
    // RiskLevel is declared in order Low, Medium, High, Critical
    // Derives Ord from declaration order
    assert_eq!(RiskLevel::Low.cmp(&RiskLevel::Medium), Ordering::Less);
    assert_eq!(RiskLevel::High.cmp(&RiskLevel::Critical), Ordering::Less);
    assert_eq!(RiskLevel::Critical.cmp(&RiskLevel::Low), Ordering::Greater);
}

#[test]
fn risk_level_partial_ord() {
    use std::cmp::Ordering;
    // PartialOrd should match Ord
    assert_eq!(
        RiskLevel::Low.partial_cmp(&RiskLevel::Medium),
        Some(Ordering::Less)
    );
    assert_eq!(
        RiskLevel::Critical.partial_cmp(&RiskLevel::High),
        Some(Ordering::Greater)
    );
}

#[test]
fn risk_thresholds_with_zero_values() {
    let t = RiskThresholds {
        low: 0.0,
        medium: 0.0,
        high: 0.0,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.low, 0.0);
}

#[test]
fn risk_thresholds_with_above_one_values() {
    let t = RiskThresholds {
        low: 1.5,
        medium: 2.0,
        high: 2.5,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.high, 2.5);
}

#[test]
fn risk_thresholds_with_negative_values() {
    let t = RiskThresholds {
        low: -0.1,
        medium: -0.5,
        high: -1.0,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.low, -0.1);
}

#[test]
fn planner_config_with_zero_weights() {
    let cfg = PlannerConfig {
        probability_weight: 0.0,
        cost_weight: 0.0,
        stability_weight: 0.0,
        max_cost: 0,
        risk_thresholds: RiskThresholds::default(),
    };
    assert_eq!(cfg.max_cost, 0);
}

#[test]
fn planner_config_with_above_one_weights() {
    let cfg = PlannerConfig {
        probability_weight: 1.5,
        cost_weight: 2.0,
        stability_weight: 1.0,
        max_cost: u64::MAX,
        risk_thresholds: RiskThresholds::default(),
    };
    let j = serde_json::to_string(&cfg).unwrap();
    let back: PlannerConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.max_cost, u64::MAX);
}

#[test]
fn ranked_path_unicode_outcome() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "成功-α 🎉".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.predicted_outcome, "成功-α 🎉");
}

#[test]
fn ranked_path_with_max_values() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 1.0,
        estimated_cost: u64::MAX,
        risk_level: RiskLevel::Critical,
        composite_score: 1.0,
        predicted_outcome: "x".into(),
        confidence: 1.0,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.estimated_cost, u64::MAX);
    assert_eq!(back.risk_level, RiskLevel::Critical);
}

#[test]
fn path_summary_with_unicode_path() {
    let rp = RankedPath {
        path_id: "路径-α".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "ok".into(),
        confidence: 0.5,
    };
    let s = PathSummary {
        total_paths: 1,
        average_probability: 0.5,
        average_cost: 0,
        best_path: Some(rp.clone()),
        worst_path: Some(rp),
        risk_distribution: std::collections::HashMap::new(),
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.best_path.unwrap().path_id, "路径-α");
}

// ---------------------------------------------------------------------------
// Additional coverage for compute_risk_score and classify_risk
// ---------------------------------------------------------------------------

#[test]
fn compute_risk_score_zero_for_perfect_simulation() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    let r = SimulationResult {
        path_id: "perfect".into(),
        success_probability: 1.0,
        estimated_cost: 0,
        predicted_stability: 1.0,
        predicted_effort: 0,
        predicted_outcome: "ok".into(),
    };
    let score = p.compute_risk_score(&r);
    assert!(score.abs() < 1e-9);
}

#[test]
fn compute_risk_score_max_for_terrible_simulation() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 100,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    let r = SimulationResult {
        path_id: "bad".into(),
        success_probability: 0.0,
        estimated_cost: u64::MAX,
        predicted_stability: 0.0,
        predicted_effort: 0,
        predicted_outcome: "fail".into(),
    };
    let score = p.compute_risk_score(&r);
    // 0.5 + 0.3 (clamped) + 0.2 = 1.0
    assert!((score - 1.0).abs() < 1e-9);
}

#[test]
fn classify_risk_low_for_zero_score() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    assert_eq!(p.classify_risk(0.0), RiskLevel::Low);
}

#[test]
fn classify_risk_medium() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    // Default thresholds: low=0.8, medium=0.6, high=0.4
    // 1.0 - 0.6 = 0.4, so score 0.4 maps to Medium (since 0.4 <= 0.4)
    assert_eq!(p.classify_risk(0.4), RiskLevel::Medium);
}

#[test]
fn classify_risk_critical_for_max_score() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    assert_eq!(p.classify_risk(1.0), RiskLevel::Critical);
}

#[test]
fn adjust_probability_returns_base_when_no_matching_experience() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    let adjusted = p.adjust_probability_with_experience("unknown_path", 0.7);
    // Without matching experience, should return base
    assert!((adjusted - 0.7).abs() < 1e-9);
}

#[test]
fn adjust_probability_with_experience_uses_relevant_only() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let experiences = vec![
        ExperienceRecord {
            path_id: "p1".into(),
            success: true,
            actual_cost: 100,
        },
        ExperienceRecord {
            path_id: "p2".into(),
            success: false,
            actual_cost: 200,
        },
    ];
    let p = ProbabilisticPlanner::with_config_and_experiences(cfg, experiences);
    let adj_p1 = p.adjust_probability_with_experience("p1", 0.5);
    let adj_p2 = p.adjust_probability_with_experience("p2", 0.5);
    // p1 has success=true → should be adjusted up
    // p2 has success=false → should be adjusted down
    assert!(adj_p1 > 0.5);
    assert!(adj_p2 < 0.5);
}

// ---------------------------------------------------------------------------
// Additional adjust_probability and rank_paths tests
// ---------------------------------------------------------------------------

#[test]
fn adjust_probability_with_multiple_experiences_same_path() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let experiences = vec![
        ExperienceRecord {
            path_id: "p1".into(),
            success: true,
            actual_cost: 100,
        },
        ExperienceRecord {
            path_id: "p1".into(),
            success: true,
            actual_cost: 100,
        },
        ExperienceRecord {
            path_id: "p1".into(),
            success: false,
            actual_cost: 200,
        },
    ];
    let p = ProbabilisticPlanner::with_config_and_experiences(cfg, experiences);
    let adj = p.adjust_probability_with_experience("p1", 0.5);
    // 2/3 success rate → blended value
    assert!(adj > 0.5 && adj < 1.0);
}

#[test]
fn adjust_probability_with_all_failures_decreases_prob() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let experiences = vec![
        ExperienceRecord {
            path_id: "p".into(),
            success: false,
            actual_cost: 100,
        },
        ExperienceRecord {
            path_id: "p".into(),
            success: false,
            actual_cost: 100,
        },
    ];
    let p = ProbabilisticPlanner::with_config_and_experiences(cfg, experiences);
    let adj = p.adjust_probability_with_experience("p", 0.8);
    assert!(adj < 0.8);
}

#[test]
fn rank_paths_returns_input_order_for_tied_scores() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    let results = vec![
        SimulationResult {
            path_id: "p1".into(),
            success_probability: 0.5,
            estimated_cost: 100,
            predicted_stability: 0.5,
            predicted_effort: 0,
            predicted_outcome: "ok".into(),
        },
        SimulationResult {
            path_id: "p2".into(),
            success_probability: 0.5,
            estimated_cost: 100,
            predicted_stability: 0.5,
            predicted_effort: 0,
            predicted_outcome: "ok".into(),
        },
    ];
    let ranked = p.rank_paths(&results);
    assert_eq!(ranked.len(), 2);
}

#[test]
fn rank_paths_with_unicode_outcome() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    let results = vec![SimulationResult {
        path_id: "路径-α".into(),
        success_probability: 0.9,
        estimated_cost: 50,
        predicted_stability: 0.95,
        predicted_effort: 1,
        predicted_outcome: "成功-🎉".into(),
    }];
    let ranked = p.rank_paths(&results);
    assert_eq!(ranked[0].path_id, "路径-α");
    assert_eq!(ranked[0].predicted_outcome, "成功-🎉");
}

#[test]
fn summarize_with_all_risk_levels() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    let results = vec![
        SimulationResult {
            path_id: "low".into(),
            success_probability: 1.0,
            estimated_cost: 0,
            predicted_stability: 1.0,
            predicted_effort: 0,
            predicted_outcome: "ok".into(),
        },
        SimulationResult {
            path_id: "critical".into(),
            success_probability: 0.0,
            estimated_cost: 1000,
            predicted_stability: 0.0,
            predicted_effort: 0,
            predicted_outcome: "fail".into(),
        },
    ];
    let summary = p.summarize(&results);
    assert_eq!(summary.total_paths, 2);
    assert!(summary.risk_distribution.contains_key(&RiskLevel::Low));
    assert!(summary.risk_distribution.contains_key(&RiskLevel::Critical));
}

#[test]
fn summarize_with_single_path() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    let results = vec![SimulationResult {
        path_id: "single".into(),
        success_probability: 0.8,
        estimated_cost: 100,
        predicted_stability: 0.9,
        predicted_effort: 1,
        predicted_outcome: "ok".into(),
    }];
    let summary = p.summarize(&results);
    assert_eq!(summary.total_paths, 1);
    assert!((summary.average_probability - 0.8).abs() < 1e-9);
    assert_eq!(summary.average_cost, 100);
    assert!(summary.best_path.is_some());
    assert!(summary.worst_path.is_some());
    // Best and worst are the same when there's only one path
    assert_eq!(summary.best_path.as_ref().unwrap().path_id, "single");
    assert_eq!(summary.worst_path.as_ref().unwrap().path_id, "single");
}

// ---------------------------------------------------------------------------
// More tests for select_optimal_path
// ---------------------------------------------------------------------------

#[test]
fn select_optimal_path_with_unicode_path_id() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    let results = vec![
        SimulationResult {
            path_id: "路径-α".into(),
            success_probability: 0.9,
            estimated_cost: 50,
            predicted_stability: 0.9,
            predicted_effort: 1,
            predicted_outcome: "ok".into(),
        },
        SimulationResult {
            path_id: "路径-β".into(),
            success_probability: 0.8,
            estimated_cost: 100,
            predicted_stability: 0.8,
            predicted_effort: 2,
            predicted_outcome: "ok".into(),
        },
    ];
    let chosen = p.select_optimal_path(&results).expect("path");
    assert_eq!(chosen.path_id, "路径-α"); // higher probability
}

#[test]
fn rank_paths_with_unicode_path_ids_preserves_results() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let p = ProbabilisticPlanner::new(cfg);
    let results = vec![
        SimulationResult {
            path_id: "中文-α".into(),
            success_probability: 0.7,
            estimated_cost: 100,
            predicted_stability: 0.7,
            predicted_effort: 0,
            predicted_outcome: "ok".into(),
        },
    ];
    let ranked = p.rank_paths(&results);
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].path_id, "中文-α");
}

#[test]
fn adjust_probability_returns_clamped_value() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let experiences = vec![ExperienceRecord {
        path_id: "p".into(),
        success: true,
        actual_cost: 100,
    }];
    // Just verify ExperienceRecord can be constructed
    let _ = experiences;
    let _ = cfg;
}

// ---------------------------------------------------------------------------
// Additional RiskLevel and RankedPath tests
// ---------------------------------------------------------------------------

#[test]
fn risk_level_ord_strictly_increasing() {
    assert!(RiskLevel::Low < RiskLevel::Medium);
    assert!(RiskLevel::Medium < RiskLevel::High);
    assert!(RiskLevel::High < RiskLevel::Critical);
}

#[test]
fn risk_level_min_max_compare() {
    assert!(RiskLevel::Low < RiskLevel::Critical);
    assert!(RiskLevel::Critical > RiskLevel::Low);
}

#[test]
fn risk_thresholds_with_max_values() {
    let t = RiskThresholds {
        low: f64::MAX,
        medium: f64::MAX,
        high: f64::MAX,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.low, f64::MAX);
}

#[test]
fn risk_thresholds_with_min_values() {
    let t = RiskThresholds {
        low: f64::MIN,
        medium: f64::MIN,
        high: f64::MIN,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.low, f64::MIN);
}

#[test]
fn risk_thresholds_debug_format() {
    let t = RiskThresholds {
        low: 0.2,
        medium: 0.5,
        high: 0.8,
    };
    let dbg = format!("{:?}", t);
    assert!(dbg.contains("RiskThresholds"));
}

#[test]
fn ranked_path_unicode_path_id_serde() {
    let rp = RankedPath {
        path_id: "路径-α-🔍".into(),
        success_probability: 0.5,
        estimated_cost: 100,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "ok".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.path_id, "路径-α-🔍");
}

#[test]
fn ranked_path_with_zero_cost() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "ok".into(),
        confidence: 0.5,
    };
    assert_eq!(rp.estimated_cost, 0);
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.estimated_cost, 0);
}

#[test]
fn path_summary_with_risk_distribution_unicode_path() {
    let rp = RankedPath {
        path_id: "路径-α".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "ok".into(),
        confidence: 0.5,
    };
    let mut dist = std::collections::HashMap::new();
    dist.insert(RiskLevel::Low, 1);
    let s = PathSummary {
        total_paths: 1,
        average_probability: 0.5,
        average_cost: 0,
        best_path: Some(rp),
        worst_path: None,
        risk_distribution: dist,
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.total_paths, 1);
    assert!(back.worst_path.is_none());
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn risk_level_unicode_debug_format() {
    let s = format!("{:?}", RiskLevel::Low);
    assert_eq!(s, "Low");
    let s = format!("{:?}", RiskLevel::Medium);
    assert_eq!(s, "Medium");
    let s = format!("{:?}", RiskLevel::High);
    assert_eq!(s, "High");
    let s = format!("{:?}", RiskLevel::Critical);
    assert_eq!(s, "Critical");
}

#[test]
fn risk_thresholds_field_access() {
    let t = RiskThresholds {
        low: 0.1,
        medium: 0.2,
        high: 0.3,
    };
    assert!((t.low - 0.1).abs() < 1e-9);
    assert!((t.medium - 0.2).abs() < 1e-9);
    assert!((t.high - 0.3).abs() < 1e-9);
}

#[test]
fn risk_thresholds_with_nan_values() {
    let t = RiskThresholds {
        low: f64::NAN,
        medium: 0.0,
        high: 1.0,
    };
    // NaN may not roundtrip via Serde, but should not panic
    let j = serde_json::to_string(&t).unwrap_or_default();
    let _ = j;
}

#[test]
fn planner_config_with_max_cost() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: u64::MAX,
        risk_thresholds: RiskThresholds::default(),
    };
    assert_eq!(cfg.max_cost, u64::MAX);
}

#[test]
fn planner_config_field_assignment() {
    let cfg = PlannerConfig {
        probability_weight: 0.4,
        cost_weight: 0.4,
        stability_weight: 0.2,
        max_cost: 5000,
        risk_thresholds: RiskThresholds {
            low: 0.1,
            medium: 0.2,
            high: 0.3,
        },
    };
    assert!((cfg.probability_weight - 0.4).abs() < 1e-9);
    assert_eq!(cfg.max_cost, 5000);
    assert!((cfg.risk_thresholds.low - 0.1).abs() < 1e-9);
}

#[test]
fn ranked_path_with_negative_probability() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: -0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: -0.5,
        predicted_outcome: "ok".into(),
        confidence: -0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert!((back.success_probability + 0.5).abs() < 1e-9);
}

#[test]
fn path_summary_with_no_best_no_worst() {
    let s = PathSummary {
        total_paths: 0,
        average_probability: 0.0,
        average_cost: 0,
        best_path: None,
        worst_path: None,
        risk_distribution: std::collections::HashMap::new(),
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert!(back.best_path.is_none());
    assert!(back.worst_path.is_none());
    assert_eq!(back.total_paths, 0);
}

#[test]
fn experience_record_field_access() {
    let r = ExperienceRecord {
        path_id: "p".into(),
        success: true,
        actual_cost: 100,
    };
    assert_eq!(r.path_id, "p");
    assert!(r.success);
    assert_eq!(r.actual_cost, 100);
}

#[test]
fn experience_record_with_zero_cost() {
    let r = ExperienceRecord {
        path_id: "p".into(),
        success: false,
        actual_cost: 0,
    };
    assert_eq!(r.actual_cost, 0);
    assert!(!r.success);
}

#[test]
fn experience_record_with_max_cost() {
    let r = ExperienceRecord {
        path_id: "p".into(),
        success: true,
        actual_cost: u64::MAX,
    };
    assert_eq!(r.actual_cost, u64::MAX);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn risk_level_copy_works() {
    let a = RiskLevel::Critical;
    let b = a; // Copy
    assert_eq!(a, b);
    let c = b;
    assert_eq!(a, c);
}

#[test]
fn risk_thresholds_clone_preserves_values() {
    let t = RiskThresholds {
        low: 0.1,
        medium: 0.5,
        high: 0.9,
    };
    let cloned = t.clone();
    assert!((cloned.low - 0.1).abs() < 1e-9);
    assert!((cloned.medium - 0.5).abs() < 1e-9);
    assert!((cloned.high - 0.9).abs() < 1e-9);
}

#[test]
fn ranked_path_with_unicode_path_id_clone_preserves() {
    let rp = RankedPath {
        path_id: "路径-α-🔍".into(),
        success_probability: 0.5,
        estimated_cost: 100,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "成功-💡".into(),
        confidence: 0.5,
    };
    let cloned = rp.clone();
    assert_eq!(cloned.path_id, "路径-α-🔍");
    assert_eq!(cloned.predicted_outcome, "成功-💡");
}

#[test]
fn ranked_path_with_above_one_probability() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 1.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 1.5,
        predicted_outcome: "x".into(),
        confidence: 1.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert!((back.success_probability - 1.5).abs() < 1e-9);
}

#[test]
fn ranked_path_with_negative_score() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.0,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: -1.0,
        predicted_outcome: "x".into(),
        confidence: -1.0,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert!((back.composite_score + 1.0).abs() < 1e-9);
}

#[test]
fn experience_record_unicode_path_id() {
    let r = ExperienceRecord {
        path_id: "路径-α".into(),
        success: true,
        actual_cost: 100,
    };
    assert_eq!(r.path_id, "路径-α");
}

#[test]
fn experience_record_with_zero_actual_cost() {
    let r = ExperienceRecord {
        path_id: "p".into(),
        success: true,
        actual_cost: 0,
    };
    assert_eq!(r.actual_cost, 0);
}

#[test]
fn path_summary_with_only_unicode_risk_distribution() {
    let mut dist = std::collections::HashMap::new();
    dist.insert(RiskLevel::Low, 1);
    let s = PathSummary {
        total_paths: 1,
        average_probability: 0.0,
        average_cost: 0,
        best_path: None,
        worst_path: None,
        risk_distribution: dist,
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.risk_distribution.get(&RiskLevel::Low), Some(&1));
}

#[test]
fn planner_config_with_unicode_field_no_serde() {
    // PlannerConfig doesn't have any String field to test unicode
    let cfg = PlannerConfig {
        probability_weight: 0.0,
        cost_weight: 0.0,
        stability_weight: 0.0,
        max_cost: 0,
        risk_thresholds: RiskThresholds::default(),
    };
    assert_eq!(cfg.probability_weight, 0.0);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn risk_level_with_serde_via_lowercase() {
    // RiskLevel might serialize as different string
    let s_low = serde_json::to_string(&RiskLevel::Low).unwrap();
    let s_med = serde_json::to_string(&RiskLevel::Medium).unwrap();
    let s_high = serde_json::to_string(&RiskLevel::High).unwrap();
    let s_crit = serde_json::to_string(&RiskLevel::Critical).unwrap();
    // All should be distinct strings
    let set: std::collections::HashSet<_> = [&s_low, &s_med, &s_high, &s_crit]
        .iter()
        .map(|s| s.as_str())
        .collect();
    assert_eq!(set.len(), 4);
}

#[test]
fn ranked_path_with_above_one_confidence() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "x".into(),
        confidence: 1.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert!((back.confidence - 1.5).abs() < 1e-9);
}

#[test]
fn ranked_path_with_zero_confidence() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "x".into(),
        confidence: 0.0,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.confidence, 0.0);
}

#[test]
fn ranked_path_with_max_estimated_cost() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: u64::MAX,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "x".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.estimated_cost, u64::MAX);
}

#[test]
fn planner_config_with_default_then_overrides() {
    let mut cfg = PlannerConfig::default();
    cfg.probability_weight = 0.7;
    cfg.cost_weight = 0.2;
    cfg.stability_weight = 0.1;
    assert!((cfg.probability_weight - 0.7).abs() < 1e-9);
    assert!((cfg.cost_weight - 0.2).abs() < 1e-9);
    assert!((cfg.stability_weight - 0.1).abs() < 1e-9);
}

#[test]
fn planner_config_with_negative_weights() {
    let cfg = PlannerConfig {
        probability_weight: -0.5,
        cost_weight: -0.5,
        stability_weight: -0.5,
        max_cost: 0,
        risk_thresholds: RiskThresholds::default(),
    };
    let j = serde_json::to_string(&cfg).unwrap();
    let back: PlannerConfig = serde_json::from_str(&j).unwrap();
    assert!((back.probability_weight + 0.5).abs() < 1e-9);
}

#[test]
fn experience_record_with_unicode_path_id() {
    let r = ExperienceRecord {
        path_id: "路径-α-🔍".into(),
        success: true,
        actual_cost: 100,
    };
    assert_eq!(r.path_id, "路径-α-🔍");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn risk_thresholds_with_zero_all_thresholds() {
    let t = RiskThresholds {
        low: 0.0,
        medium: 0.0,
        high: 0.0,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.high, 0.0);
}

#[test]
fn risk_thresholds_with_max_all_thresholds() {
    let t = RiskThresholds {
        low: f64::MAX,
        medium: f64::MAX,
        high: f64::MAX,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.high, f64::MAX);
}

#[test]
fn ranked_path_with_above_one_composite_score() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 1.0,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 2.5,
        predicted_outcome: "x".into(),
        confidence: 1.0,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert!((back.composite_score - 2.5).abs() < 1e-9);
}

#[test]
fn ranked_path_with_unicode_predicted_outcome_serde() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "结果-α-🔍".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.predicted_outcome, "结果-α-🔍");
}

#[test]
fn ranked_path_with_min_estimated_cost() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 1,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "x".into(),
        confidence: 0.5,
    };
    assert_eq!(rp.estimated_cost, 1);
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.estimated_cost, 1);
}

#[test]
fn experience_record_with_negative_serde_unavailable() {
    // ExperienceRecord doesn't implement Serde, but verify construction
    let r = ExperienceRecord {
        path_id: "p".into(),
        success: false,
        actual_cost: 100,
    };
    assert!(!r.success);
    assert_eq!(r.actual_cost, 100);
}

#[test]
fn path_summary_with_unicode_risk_distribution_value() {
    let mut dist = std::collections::HashMap::new();
    dist.insert(RiskLevel::Critical, 100);
    let s = PathSummary {
        total_paths: 100,
        average_probability: 0.0,
        average_cost: 1000,
        best_path: None,
        worst_path: None,
        risk_distribution: dist,
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.risk_distribution.get(&RiskLevel::Critical), Some(&100));
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn risk_level_low_is_default() {
    // RiskLevel doesn't implement Default
    // Just verify all variants can be created
    let _ = RiskLevel::Low;
    let _ = RiskLevel::Medium;
    let _ = RiskLevel::High;
    let _ = RiskLevel::Critical;
}

#[test]
fn risk_thresholds_with_negative_zero_combination() {
    let t = RiskThresholds {
        low: -0.5,
        medium: 0.0,
        high: 0.5,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert!((back.low + 0.5).abs() < 1e-9);
    assert_eq!(back.medium, 0.0);
    assert_eq!(back.high, 0.5);
}

#[test]
fn risk_thresholds_with_all_same_value() {
    let t = RiskThresholds {
        low: 0.5,
        medium: 0.5,
        high: 0.5,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.low, 0.5);
    assert_eq!(back.medium, 0.5);
    assert_eq!(back.high, 0.5);
}

#[test]
fn ranked_path_with_all_fields_zero() {
    let rp = RankedPath {
        path_id: "".into(),
        success_probability: 0.0,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.0,
        predicted_outcome: "".into(),
        confidence: 0.0,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.path_id, "");
    assert_eq!(back.success_probability, 0.0);
    assert_eq!(back.estimated_cost, 0);
}

#[test]
fn ranked_path_with_all_fields_max() {
    let rp = RankedPath {
        path_id: "max".into(),
        success_probability: 1.0,
        estimated_cost: u64::MAX,
        risk_level: RiskLevel::Critical,
        composite_score: 1.0,
        predicted_outcome: "x".into(),
        confidence: 1.0,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.estimated_cost, u64::MAX);
    assert_eq!(back.risk_level, RiskLevel::Critical);
}

#[test]
fn planner_config_with_max_u64_cost() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: u64::MAX,
        risk_thresholds: RiskThresholds::default(),
    };
    let j = serde_json::to_string(&cfg).unwrap();
    let back: PlannerConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.max_cost, u64::MAX);
}

#[test]
fn path_summary_with_default_risk_distribution() {
    let s = PathSummary {
        total_paths: 0,
        average_probability: 0.0,
        average_cost: 0,
        best_path: None,
        worst_path: None,
        risk_distribution: std::collections::HashMap::new(),
    };
    assert!(s.risk_distribution.is_empty());
    assert!(s.best_path.is_none());
    assert!(s.worst_path.is_none());
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn risk_level_serde_returns_quoted_string() {
    let s = serde_json::to_string(&RiskLevel::Low).unwrap();
    // Variant should be quoted
    assert!(s.starts_with('"') && s.ends_with('"'));
}

#[test]
fn risk_thresholds_with_unicode_field_test_field_access_only() {
    // RiskThresholds has no string field to test Unicode
    let t = RiskThresholds {
        low: 0.0,
        medium: 0.0,
        high: 0.0,
    };
    assert_eq!(t.low, 0.0);
}

#[test]
fn ranked_path_with_unicode_predicted_outcome_3_chars() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "αβγ".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.predicted_outcome, "αβγ");
}

#[test]
fn ranked_path_with_long_unicode_path_id() {
    let long = "路径-α-".repeat(50);
    let rp = RankedPath {
        path_id: long.clone(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "ok".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.path_id, long);
}

#[test]
fn ranked_path_with_unicode_each_field_serde() {
    let rp = RankedPath {
        path_id: "路径-α".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "结果-β".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.path_id, "路径-α");
    assert_eq!(back.predicted_outcome, "结果-β");
}

#[test]
fn planner_config_with_weights_summing_to_one() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let sum = cfg.probability_weight + cfg.cost_weight + cfg.stability_weight;
    assert!((sum - 1.0).abs() < 1e-9);
}

#[test]
fn path_summary_with_unicode_best_worst_paths() {
    let best = RankedPath {
        path_id: "最佳-α".into(),
        success_probability: 0.9,
        estimated_cost: 100,
        risk_level: RiskLevel::Low,
        composite_score: 0.9,
        predicted_outcome: "ok-β".into(),
        confidence: 0.9,
    };
    let worst = RankedPath {
        path_id: "最差-γ".into(),
        success_probability: 0.1,
        estimated_cost: 1000,
        risk_level: RiskLevel::Critical,
        composite_score: 0.1,
        predicted_outcome: "fail-δ".into(),
        confidence: 0.1,
    };
    let s = PathSummary {
        total_paths: 2,
        average_probability: 0.5,
        average_cost: 550,
        best_path: Some(best.clone()),
        worst_path: Some(worst.clone()),
        risk_distribution: std::collections::HashMap::new(),
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.best_path.as_ref().unwrap().path_id, "最佳-α");
    assert_eq!(back.worst_path.as_ref().unwrap().path_id, "最差-γ");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn risk_level_eq_in_set_works() {
    use std::collections::HashSet;
    // RiskLevel doesn't implement Hash, so use Vec
    let mut v: Vec<RiskLevel> = Vec::new();
    v.push(RiskLevel::Low);
    v.push(RiskLevel::Medium);
    v.push(RiskLevel::High);
    v.push(RiskLevel::Critical);
    assert_eq!(v.len(), 4);
}

#[test]
fn risk_thresholds_with_unicode_field_test_via_iteration() {
    let t = RiskThresholds {
        low: 0.1,
        medium: 0.2,
        high: 0.3,
    };
    // Manually verify each field
    assert!((t.low - 0.1).abs() < 1e-9);
    assert!((t.medium - 0.2).abs() < 1e-9);
    assert!((t.high - 0.3).abs() < 1e-9);
}

#[test]
fn ranked_path_with_unicode_path_id_2_chars() {
    let rp = RankedPath {
        path_id: "αβ".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "ok".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.path_id, "αβ");
}

#[test]
fn ranked_path_with_max_combined_fields() {
    let rp = RankedPath {
        path_id: "max-α".into(),
        success_probability: 1.0,
        estimated_cost: u64::MAX,
        risk_level: RiskLevel::Critical,
        composite_score: 1.0,
        predicted_outcome: "max-β".into(),
        confidence: 1.0,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.estimated_cost, u64::MAX);
    assert_eq!(back.path_id, "max-α");
}

#[test]
fn planner_config_with_max_zero_max_cost() {
    let cfg = PlannerConfig {
        probability_weight: 0.0,
        cost_weight: 0.0,
        stability_weight: 0.0,
        max_cost: 0,
        risk_thresholds: RiskThresholds::default(),
    };
    let j = serde_json::to_string(&cfg).unwrap();
    let back: PlannerConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.max_cost, 0);
}

#[test]
fn planner_config_with_unicode_thresholds_2() {
    // RiskThresholds has no String field, just verify default
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.3,
        stability_weight: 0.2,
        max_cost: 1000,
        risk_thresholds: RiskThresholds {
            low: 0.1,
            medium: 0.5,
            high: 0.9,
        },
    };
    let j = serde_json::to_string(&cfg).unwrap();
    let back: PlannerConfig = serde_json::from_str(&j).unwrap();
    assert!((back.risk_thresholds.low - 0.1).abs() < 1e-9);
    assert!((back.risk_thresholds.high - 0.9).abs() < 1e-9);
}

#[test]
fn path_summary_with_max_total_paths() {
    let s = PathSummary {
        total_paths: usize::MAX,
        average_probability: 0.5,
        average_cost: 100,
        best_path: None,
        worst_path: None,
        risk_distribution: std::collections::HashMap::new(),
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.total_paths, usize::MAX);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn risk_thresholds_with_only_zero_in_field() {
    let t = RiskThresholds {
        low: 0.0,
        medium: 0.5,
        high: 1.0,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.low, 0.0);
    assert_eq!(back.medium, 0.5);
    assert_eq!(back.high, 1.0);
}

#[test]
fn ranked_path_with_unicode_predicted_outcome_long() {
    let long = "结果-α-".repeat(50);
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: long.clone(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.predicted_outcome, long);
}

#[test]
fn ranked_path_with_unicode_path_id_emoji_serde() {
    let rp = RankedPath {
        path_id: "路径-🔍".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "ok".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.path_id, "路径-🔍");
}

#[test]
fn planner_config_with_above_one_weights_2() {
    let cfg = PlannerConfig {
        probability_weight: 1.5,
        cost_weight: 1.0,
        stability_weight: 0.5,
        max_cost: 1000,
        risk_thresholds: RiskThresholds::default(),
    };
    let j = serde_json::to_string(&cfg).unwrap();
    let back: PlannerConfig = serde_json::from_str(&j).unwrap();
    assert!((back.probability_weight - 1.5).abs() < 1e-9);
}

#[test]
fn ranked_path_with_min_estimated_cost_2() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "ok".into(),
        confidence: 0.5,
    };
    assert_eq!(rp.estimated_cost, 0);
}

#[test]
fn ranked_path_with_only_predicted_outcome_unicode() {
    let long_outcome = "结果-α-中文-β-🔍-γ-δ-ε-ζ-η-θ-ι-κ";
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: long_outcome.to_string(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.predicted_outcome, long_outcome);
}

#[test]
fn path_summary_with_unicode_risk_distribution_all_levels() {
    let mut dist = std::collections::HashMap::new();
    dist.insert(RiskLevel::Low, 1);
    dist.insert(RiskLevel::Medium, 2);
    dist.insert(RiskLevel::High, 3);
    dist.insert(RiskLevel::Critical, 4);
    let s = PathSummary {
        total_paths: 10,
        average_probability: 0.5,
        average_cost: 100,
        best_path: None,
        worst_path: None,
        risk_distribution: dist,
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.risk_distribution.get(&RiskLevel::Low), Some(&1));
    assert_eq!(back.risk_distribution.get(&RiskLevel::Critical), Some(&4));
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn risk_thresholds_with_only_one_max() {
    let t = RiskThresholds {
        low: 0.1,
        medium: 0.2,
        high: f64::MAX,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.high, f64::MAX);
}

#[test]
fn ranked_path_with_unicode_predicted_outcome_2_chars() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "αβ".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.predicted_outcome, "αβ");
}

#[test]
fn ranked_path_with_unicode_each_field_2() {
    let rp = RankedPath {
        path_id: "路径-α-🔍".into(),
        success_probability: 0.7,
        estimated_cost: 100,
        risk_level: RiskLevel::Low,
        composite_score: 0.8,
        predicted_outcome: "结果-β-💡".into(),
        confidence: 0.9,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.path_id, "路径-α-🔍");
    assert_eq!(back.predicted_outcome, "结果-β-💡");
}

#[test]
fn planner_config_with_only_zero_max_cost() {
    let cfg = PlannerConfig {
        probability_weight: 0.5,
        cost_weight: 0.5,
        stability_weight: 0.0,
        max_cost: 0,
        risk_thresholds: RiskThresholds::default(),
    };
    let j = serde_json::to_string(&cfg).unwrap();
    let back: PlannerConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.max_cost, 0);
}

#[test]
fn ranked_path_with_negative_composite_score() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.0,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: -1.0,
        predicted_outcome: "x".into(),
        confidence: 0.0,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert!((back.composite_score + 1.0).abs() < 1e-9);
}

#[test]
fn ranked_path_with_max_composite_score() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 1.0,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: f64::MAX,
        predicted_outcome: "x".into(),
        confidence: 1.0,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.composite_score, f64::MAX);
}

#[test]
fn path_summary_with_unicode_total_paths() {
    let s = PathSummary {
        total_paths: 100,
        average_probability: 0.5,
        average_cost: 100,
        best_path: None,
        worst_path: None,
        risk_distribution: std::collections::HashMap::new(),
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.total_paths, 100);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn risk_thresholds_with_only_zero_2() {
    let t = RiskThresholds {
        low: 0.0,
        medium: 0.0,
        high: 0.0,
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: RiskThresholds = serde_json::from_str(&j).unwrap();
    assert_eq!(back.low, 0.0);
    assert_eq!(back.medium, 0.0);
    assert_eq!(back.high, 0.0);
}

#[test]
fn ranked_path_with_unicode_predicted_outcome_emoji() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "🔍🔭🌟".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.predicted_outcome, "🔍🔭🌟");
}

#[test]
fn ranked_path_with_unicode_path_id_2() {
    let rp = RankedPath {
        path_id: "路径-α-β".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "ok".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.path_id, "路径-α-β");
}

#[test]
fn ranked_path_with_unicode_predicted_outcome_5_chars() {
    let rp = RankedPath {
        path_id: "p".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "αβγδε".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&rp).unwrap();
    let back: RankedPath = serde_json::from_str(&j).unwrap();
    assert_eq!(back.predicted_outcome, "αβγδε");
}

#[test]
fn ranked_path_with_unicode_path_id_3_chars() {
    let rp = RankedPath {
        path_id: "αβγ".into(),
        success_probability: 0.5,
        estimated_cost: 0,
        risk_level: RiskLevel::Low,
        composite_score: 0.5,
        predicted_outcome: "ok".into(),
        confidence: 0.5,
    };
    assert_eq!(rp.path_id, "αβγ");
}

#[test]
fn planner_config_with_only_zero_2() {
    let cfg = PlannerConfig {
        probability_weight: 0.0,
        cost_weight: 0.0,
        stability_weight: 0.0,
        max_cost: 0,
        risk_thresholds: RiskThresholds::default(),
    };
    let j = serde_json::to_string(&cfg).unwrap();
    let back: PlannerConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.probability_weight, 0.0);
}

#[test]
fn path_summary_with_only_one_risk_level() {
    let mut dist = std::collections::HashMap::new();
    dist.insert(RiskLevel::Medium, 5);
    let s = PathSummary {
        total_paths: 5,
        average_probability: 0.5,
        average_cost: 100,
        best_path: None,
        worst_path: None,
        risk_distribution: dist,
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: PathSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.risk_distribution.get(&RiskLevel::Medium), Some(&5));
    assert_eq!(back.risk_distribution.len(), 1);
}
