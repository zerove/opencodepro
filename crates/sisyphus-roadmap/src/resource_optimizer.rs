// ---------------------------------------------------------------------------
// Phase 3 — Resource / Cost Global Optimisation
//
// Analyses execution history to project remaining costs, identify
// bottlenecks, and suggest parallelisation opportunities.  Operates
// on recorded execution outcomes without requiring LLM calls.
// ---------------------------------------------------------------------------

use crate::ExecutionOutcome;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A cost projection for the remaining work in a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostProjection {
    pub total_goals: usize,
    pub completed_goals: usize,
    pub remaining_goals: usize,
    pub avg_cost_per_goal: f64,
    pub avg_duration_per_goal_secs: f64,
    pub projected_remaining_cost: f64,
    pub projected_remaining_duration_secs: f64,
    pub confidence: f64,
}

/// A detected bottleneck in the execution pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bottleneck {
    pub goal_id: String,
    pub description: String,
    pub failure_count: usize,
    pub avg_duration_secs: f64,
    pub blocking_count: usize,
    pub severity: String, // "low", "medium", "high"
}

/// An opportunity to execute goals in parallel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelizationOpportunity {
    pub goal_ids: Vec<String>,
    pub rationale: String,
    pub estimated_time_savings_secs: f64,
}

/// Per-goal resource estimate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalResourceEstimate {
    pub goal_id: String,
    pub predicted_cost: f64,
    pub predicted_duration_secs: f64,
    pub confidence: f64,
    pub historical_samples: usize,
}

// ---------------------------------------------------------------------------
// ResourceOptimizer
// ---------------------------------------------------------------------------

/// Analytical engine for resource and cost optimisation.
///
/// Uses historical execution data to project costs, identify bottlenecks,
/// and suggest optimisations — all without LLM calls.
pub struct ResourceOptimizer;

impl Default for ResourceOptimizer {
    fn default() -> Self {
        Self
    }
}

impl ResourceOptimizer {
    pub fn new() -> Self {
        Self
    }

    /// Project remaining costs for the project based on historical outcomes.
    ///
    /// Uses average cost and duration from completed goals to estimate
    /// what remains. Confidence increases with more historical data.
    pub fn project_costs(
        &self,
        outcomes: &[ExecutionOutcome],
        total_goals: usize,
        completed_goals: usize,
    ) -> CostProjection {
        let completed_outcomes: Vec<&ExecutionOutcome> =
            outcomes.iter().filter(|o| o.success).collect();

        let (avg_cost, avg_duration) = if completed_outcomes.is_empty() {
            // No historical data — use sensible defaults
            (10.0, 60.0)
        } else {
            let total_cost: f64 = completed_outcomes
                .iter()
                .map(|o| if o.success { o.actual_cost } else { 0.0 })
                .sum();
            let total_duration: f64 = completed_outcomes.iter().map(|o| o.duration_secs).sum();
            let count = completed_outcomes.len() as f64;
            (total_cost / count, total_duration / count)
        };

        let remaining = total_goals.saturating_sub(completed_goals);
        let confidence =
            (completed_outcomes.len() as f64 / (total_goals.max(1) as f64)).clamp(0.1, 1.0);

        CostProjection {
            total_goals,
            completed_goals,
            remaining_goals: remaining,
            avg_cost_per_goal: avg_cost,
            avg_duration_per_goal_secs: avg_duration,
            projected_remaining_cost: avg_cost * remaining as f64,
            projected_remaining_duration_secs: avg_duration * remaining as f64,
            confidence,
        }
    }

    /// Identify bottlenecks in the execution pipeline.
    ///
    /// Bottlenecks are goals that:
    /// - Have a high failure rate (retried frequently)
    /// - Take significantly longer than average
    /// - Block many other goals (high out-degree in dependency graph)
    pub fn find_bottlenecks(&self, outcomes: &[ExecutionOutcome]) -> Vec<Bottleneck> {
        if outcomes.is_empty() {
            return vec![];
        }

        // Group outcomes by goal_id
        let mut by_goal: HashMap<String, Vec<&ExecutionOutcome>> = HashMap::new();
        for o in outcomes {
            by_goal.entry(o.goal_id.clone()).or_default().push(o);
        }

        // Calculate average duration of successful outcomes for baseline
        let successful_durations: Vec<f64> = outcomes
            .iter()
            .filter(|o| o.success)
            .map(|o| o.duration_secs)
            .collect();
        let avg_duration: f64 = if successful_durations.is_empty() {
            60.0
        } else {
            successful_durations.iter().sum::<f64>() / successful_durations.len() as f64
        };

        let mut bottlenecks: Vec<Bottleneck> = Vec::new();

        for (goal_id, goal_outcomes) in &by_goal {
            let failure_count = goal_outcomes.iter().filter(|o| !o.success).count();
            let success_durations: Vec<f64> = goal_outcomes
                .iter()
                .filter(|o| o.success)
                .map(|o| o.duration_secs)
                .collect();
            let avg_goal_duration: f64 = if success_durations.is_empty() {
                0.0
            } else {
                success_durations.iter().sum::<f64>() / success_durations.len() as f64
            };

            // Determine severity
            let severity = if failure_count >= 3 {
                "high"
            } else if failure_count >= 2 || avg_goal_duration > avg_duration * 3.0 {
                "medium"
            } else if failure_count >= 1 || avg_goal_duration > avg_duration * 2.0 {
                "low"
            } else {
                continue; // not a bottleneck
            };

            let description = if failure_count >= 3 {
                format!(
                    "Repeated failures ({}) — consider splitting or redesigning",
                    failure_count
                )
            } else if avg_goal_duration > avg_duration * 3.0 {
                format!(
                    "Takes {:.1}x longer than average ({:.0}s vs {:.0}s avg)",
                    avg_goal_duration / avg_duration,
                    avg_goal_duration,
                    avg_duration,
                )
            } else {
                format!(
                    "Moderate issues ({} failures, {:.0}s avg duration)",
                    failure_count, avg_goal_duration
                )
            };

            bottlenecks.push(Bottleneck {
                goal_id: goal_id.clone(),
                description,
                failure_count,
                avg_duration_secs: avg_goal_duration,
                blocking_count: 0, // would need dependency graph
                severity: severity.to_string(),
            });
        }

        // Sort by severity (high first) then by failure count
        bottlenecks.sort_by(|a, b| {
            let severity_order = |s: &str| match s {
                "high" => 0,
                "medium" => 1,
                "low" => 2,
                _ => 3,
            };
            let a_order = severity_order(&a.severity);
            let b_order = severity_order(&b.severity);
            a_order
                .cmp(&b_order)
                .then_with(|| b.failure_count.cmp(&a.failure_count))
        });

        bottlenecks
    }

    /// Suggest goals that could be executed in parallel.
    ///
    /// Uses simple heuristics: goals that have no mutual dependencies,
    /// similar durations, and no shared failure patterns can be parallelised.
    pub fn suggest_parallel_execution(
        &self,
        outcomes: &[ExecutionOutcome],
    ) -> Vec<ParallelizationOpportunity> {
        if outcomes.len() < 4 {
            return vec![];
        }

        // Group successful outcomes by milestone for parallelization candidates
        let mut by_milestone: HashMap<String, Vec<&ExecutionOutcome>> = HashMap::new();
        for o in outcomes.iter().filter(|o| o.success) {
            by_milestone
                .entry(o.milestone_id.clone())
                .or_default()
                .push(o);
        }

        let mut opportunities: Vec<ParallelizationOpportunity> = Vec::new();

        for ms_outcomes in by_milestone.values() {
            if ms_outcomes.len() < 2 {
                continue;
            }

            // If we have multiple goals in the same milestone that were
            // executed successfully, they can likely be parallelised
            let goal_ids: Vec<String> = ms_outcomes.iter().map(|o| o.goal_id.clone()).collect();

            // Estimate savings using Amdahl's law approximation
            let total_time: f64 = ms_outcomes.iter().map(|o| o.duration_secs).sum();
            let parallel_time = total_time / ms_outcomes.len().min(4) as f64;
            let savings = (total_time - parallel_time).max(0.0);

            if savings > 10.0 && goal_ids.len() >= 2 {
                opportunities.push(ParallelizationOpportunity {
                    goal_ids,
                    rationale: format!(
                        "{} goals in same milestone — estimated {:.0}s savings with parallel execution",
                        ms_outcomes.len(),
                        savings,
                    ),
                    estimated_time_savings_secs: savings,
                });
            }
        }

        opportunities
    }

    /// Generate per-goal resource estimates based on historical data.
    ///
    /// For goals with historical outcomes, uses the average of past runs.
    /// For goals without history, estimates using project-wide averages.
    pub fn estimate_goal_resources(
        &self,
        outcomes: &[ExecutionOutcome],
        goal_ids: &[String],
    ) -> Vec<GoalResourceEstimate> {
        // Build per-goal historical stats
        let mut cost_stats: HashMap<String, Vec<f64>> = HashMap::new();
        let mut duration_stats: HashMap<String, Vec<f64>> = HashMap::new();
        for o in outcomes.iter().filter(|o| o.success) {
            cost_stats
                .entry(o.goal_id.clone())
                .or_default()
                .push(o.actual_cost);
            duration_stats
                .entry(o.goal_id.clone())
                .or_default()
                .push(o.duration_secs);
        }

        // Project-wide averages for goals without history
        let all_costs: Vec<f64> = cost_stats
            .values()
            .flat_map(|v| v.iter())
            .copied()
            .collect();
        let all_durations: Vec<f64> = duration_stats
            .values()
            .flat_map(|v| v.iter())
            .copied()
            .collect();
        let global_avg_cost = if all_costs.is_empty() {
            10.0
        } else {
            all_costs.iter().sum::<f64>() / all_costs.len() as f64
        };
        let global_avg_dur = if all_durations.is_empty() {
            60.0
        } else {
            all_durations.iter().sum::<f64>() / all_durations.len() as f64
        };

        goal_ids
            .iter()
            .map(|gid| {
                if let (Some(costs), Some(durs)) = (cost_stats.get(gid), duration_stats.get(gid)) {
                    let avg_cost = costs.iter().sum::<f64>() / costs.len() as f64;
                    let avg_dur = durs.iter().sum::<f64>() / durs.len() as f64;
                    GoalResourceEstimate {
                        goal_id: gid.clone(),
                        predicted_cost: avg_cost,
                        predicted_duration_secs: avg_dur,
                        confidence: (costs.len() as f64 / 5.0).min(1.0),
                        historical_samples: costs.len(),
                    }
                } else {
                    GoalResourceEstimate {
                        goal_id: gid.clone(),
                        predicted_cost: global_avg_cost,
                        predicted_duration_secs: global_avg_dur,
                        confidence: 0.3,
                        historical_samples: 0,
                    }
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_outcome(
        goal_id: &str,
        milestone_id: &str,
        success: bool,
        timestamp: u64,
    ) -> ExecutionOutcome {
        ExecutionOutcome {
            goal_id: goal_id.into(),
            project_id: "p1".into(),
            milestone_id: milestone_id.into(),
            phase_id: "ph1".into(),
            success,
            deviation_score: None,
            error: if success { None } else { Some("error".into()) },
            timestamp,
            actual_cost: 1.0,
            duration_secs: timestamp as f64,
        }
    }

    #[test]
    fn test_project_costs_empty() {
        let opt = ResourceOptimizer::new();
        let projection = opt.project_costs(&[], 10, 0);
        assert_eq!(projection.total_goals, 10);
        assert_eq!(projection.remaining_goals, 10);
        assert!(projection.confidence >= 0.1);
    }

    #[test]
    fn test_project_costs_with_history() {
        let opt = ResourceOptimizer::new();
        let outcomes = vec![
            make_outcome("g1", "ms1", true, 30),
            make_outcome("g2", "ms1", true, 60),
            make_outcome("g3", "ms1", true, 90),
        ];
        let projection = opt.project_costs(&outcomes, 6, 3);
        assert_eq!(projection.total_goals, 6);
        assert_eq!(projection.completed_goals, 3);
        assert_eq!(projection.remaining_goals, 3);
        assert!((projection.avg_duration_per_goal_secs - 60.0).abs() < 0.01);
        assert!((projection.projected_remaining_duration_secs - 180.0).abs() < 0.01);
    }

    #[test]
    fn test_find_bottlenecks_empty() {
        let opt = ResourceOptimizer::new();
        let bottlenecks = opt.find_bottlenecks(&[]);
        assert!(bottlenecks.is_empty());
    }

    #[test]
    fn test_find_bottlenecks_with_failures() {
        let opt = ResourceOptimizer::new();
        let outcomes = vec![
            make_outcome("g1", "ms1", true, 30),
            make_outcome("g1", "ms1", false, 10),
            make_outcome("g1", "ms1", false, 15),
            make_outcome("g1", "ms1", false, 20),
            make_outcome("g2", "ms1", true, 30),
        ];
        let bottlenecks = opt.find_bottlenecks(&outcomes);
        assert!(!bottlenecks.is_empty());
        // g1 has 3 failures -> "high" severity, should be first
        assert_eq!(bottlenecks[0].goal_id, "g1");
        assert_eq!(bottlenecks[0].severity, "high");
    }

    #[test]
    fn test_suggest_parallel_execution() {
        let opt = ResourceOptimizer::new();
        let outcomes = vec![
            make_outcome("g1", "ms1", true, 30),
            make_outcome("g2", "ms1", true, 60),
            make_outcome("g3", "ms1", true, 90),
            make_outcome("g4", "ms2", true, 45),
        ];
        let suggestions = opt.suggest_parallel_execution(&outcomes);
        // ms1 has 3 goals -> parallelization opportunity
        assert!(suggestions.iter().any(|s| s.goal_ids.len() >= 2));
    }

    #[test]
    fn test_estimate_goal_resources() {
        let opt = ResourceOptimizer::new();
        let outcomes = vec![
            make_outcome("g1", "ms1", true, 30),
            make_outcome("g1", "ms1", true, 60),
            make_outcome("g2", "ms1", true, 90),
        ];
        let estimates =
            opt.estimate_goal_resources(&outcomes, &["g1".into(), "g2".into(), "g3".into()]);
        assert_eq!(estimates.len(), 3);

        // g1 has 2 samples
        let g1 = estimates.iter().find(|e| e.goal_id == "g1").unwrap();
        assert!((g1.predicted_duration_secs - 45.0).abs() < 0.01);
        assert_eq!(g1.historical_samples, 2);

        // g3 has no samples -> uses global average
        let g3 = estimates.iter().find(|e| e.goal_id == "g3").unwrap();
        assert_eq!(g3.historical_samples, 0);
        assert!(g3.confidence < 0.5);
    }
}
