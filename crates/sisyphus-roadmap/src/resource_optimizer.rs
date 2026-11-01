// ---------------------------------------------------------------------------
// 阶段 3 — 资源/成本全局优化
//
// 分析执行历史以预测剩余成本、识别瓶颈，
// 并建议并行化机会。仅基于已记录的执行结果，
// 无需调用 LLM。
// ---------------------------------------------------------------------------

use crate::{
    Bottleneck, CostProjection, ExecutionOutcome, GoalResourceEstimate, ParallelizationOpportunity,
};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// ResourceOptimizer（资源优化器）
// ---------------------------------------------------------------------------

/// 资源与成本优化的分析引擎。
///
/// 使用历史执行数据预测成本、识别瓶颈，
/// 并建议优化 — 全程无需 LLM 调用。
#[derive(Clone)]
pub struct ResourceOptimizer;

impl Default for ResourceOptimizer {
    fn default() -> Self {
        Self
    }
}

impl ResourceOptimizer {
    /// New.
    pub fn new() -> Self {
        Self
    }

    /// 基于历史结果预测项目剩余成本。
    ///
    /// 使用已完成目标的平均成本与耗时来估算
    /// 剩余部分。历史数据越多，置信度越高。
    pub fn project_costs(
        &self,
        outcomes: &[ExecutionOutcome],
        total_goals: usize,
        completed_goals: usize,
    ) -> CostProjection {
        let completed_outcomes: Vec<&ExecutionOutcome> =
            outcomes.iter().filter(|o| o.success).collect();

        let (avg_cost, avg_duration) = if completed_outcomes.is_empty() {
            // 无历史数据 — 使用合理默认值
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

    /// 识别执行流水线中的瓶颈。
    ///
    /// 瓶颈指具备以下特征的目标：
    /// - 失败率高（频繁重试）
    /// - 耗时显著高于平均
    /// - 阻塞大量其他目标（依赖图中出度高）
    pub fn find_bottlenecks(&self, outcomes: &[ExecutionOutcome]) -> Vec<Bottleneck> {
        if outcomes.is_empty() {
            return vec![];
        }

        // 按 goal_id 分组结果
        let mut by_goal: HashMap<String, Vec<&ExecutionOutcome>> = HashMap::new();
        for o in outcomes {
            by_goal.entry(o.goal_id.clone()).or_default().push(o);
        }

        // 计算成功结果的平均耗时作为基线
        let successful_durations: Vec<f64> =
            outcomes.iter().filter(|o| o.success).map(|o| o.duration_secs).collect();
        let avg_duration: f64 = if successful_durations.is_empty() {
            60.0
        } else {
            successful_durations.iter().sum::<f64>() / successful_durations.len() as f64
        };

        let mut bottlenecks: Vec<Bottleneck> = Vec::new();

        for (goal_id, goal_outcomes) in &by_goal {
            let failure_count = goal_outcomes.iter().filter(|o| !o.success).count();
            let success_durations: Vec<f64> =
                goal_outcomes.iter().filter(|o| o.success).map(|o| o.duration_secs).collect();
            let avg_goal_duration: f64 = if success_durations.is_empty() {
                0.0
            } else {
                success_durations.iter().sum::<f64>() / success_durations.len() as f64
            };

            // 确定严重程度
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

        // 先按严重程度（高在前）再按失败次数排序
        bottlenecks.sort_by(|a, b| {
            let severity_order = |s: &str| match s {
                "high" => 0,
                "medium" => 1,
                "low" => 2,
                _ => 3,
            };
            let a_order = severity_order(&a.severity);
            let b_order = severity_order(&b.severity);
            a_order.cmp(&b_order).then_with(|| b.failure_count.cmp(&a.failure_count))
        });

        bottlenecks
    }

    /// 建议可并行执行的目标。
    ///
    /// 使用简单启发式：无相互依赖、耗时相近、
    /// 且无共同失败模式的目标可以并行。
    pub fn suggest_parallel_execution(
        &self,
        outcomes: &[ExecutionOutcome],
    ) -> Vec<ParallelizationOpportunity> {
        if outcomes.len() < 4 {
            return vec![];
        }

        // 按里程碑对成功结果分组，作为并行化候选
        let mut by_milestone: HashMap<String, Vec<&ExecutionOutcome>> = HashMap::new();
        for o in outcomes.iter().filter(|o| o.success) {
            by_milestone.entry(o.milestone_id.clone()).or_default().push(o);
        }

        let mut opportunities: Vec<ParallelizationOpportunity> = Vec::new();

        for ms_outcomes in by_milestone.values() {
            if ms_outcomes.len() < 2 {
                continue;
            }

            // 若同一里程碑内有多个成功执行的目标，
            // 它们很可能可以并行化
            let goal_ids: Vec<String> = ms_outcomes.iter().map(|o| o.goal_id.clone()).collect();

            // 使用阿姆达尔定律近似估算节省量
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

    /// 基于历史数据生成每个目标的资源估算。
    ///
    /// 有历史结果的目标使用过去运行的平均值。
    /// 无历史的目标使用项目级平均值估算。
    pub fn estimate_goal_resources(
        &self,
        outcomes: &[ExecutionOutcome],
        goal_ids: &[String],
    ) -> Vec<GoalResourceEstimate> {
        // 构建每个目标的历史统计
        let mut cost_stats: HashMap<String, Vec<f64>> = HashMap::new();
        let mut duration_stats: HashMap<String, Vec<f64>> = HashMap::new();
        for o in outcomes.iter().filter(|o| o.success) {
            cost_stats.entry(o.goal_id.clone()).or_default().push(o.actual_cost);
            duration_stats.entry(o.goal_id.clone()).or_default().push(o.duration_secs);
        }

        // 无历史目标使用项目级平均值
        let all_costs: Vec<f64> = cost_stats.values().flat_map(|v| v.iter()).copied().collect();
        let all_durations: Vec<f64> =
            duration_stats.values().flat_map(|v| v.iter()).copied().collect();
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
        // g1 有 3 次失败 → "high" 严重度，应排第一
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
        // ms1 有 3 个目标 → 并行化机会
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

        // g1 有 2 个样本
        let g1 = estimates.iter().find(|e| e.goal_id == "g1").unwrap();
        assert!((g1.predicted_duration_secs - 45.0).abs() < 0.01);
        assert_eq!(g1.historical_samples, 2);

        // g3 无样本 → 使用全局平均值
        let g3 = estimates.iter().find(|e| e.goal_id == "g3").unwrap();
        assert_eq!(g3.historical_samples, 0);
        assert!(g3.confidence < 0.5);
    }
}
