//! 概率规划器 — 蒙特卡洛路径排序与最优选择。
//! 使用仿真结果对执行路径排序，并综合考虑成功概率、
//! 成本与风险权衡选出最优路径。

use crate::simulation::SimulationResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 路径的风险级别
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// 带已计算指标的排序执行路径
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedPath {
    pub path_id: String,
    pub success_probability: f64,
    pub estimated_cost: u64,
    pub risk_level: RiskLevel,
    pub composite_score: f64,
    pub predicted_outcome: String,
    pub confidence: f64,
}

/// 概率规划器的配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerConfig {
    /// 综合得分中成功概率的权重（0.0-1.0）
    pub probability_weight: f64,
    /// 综合得分中成本的权重（0.0-1.0）
    pub cost_weight: f64,
    /// 综合得分中稳定性的权重（0.0-1.0）
    pub stability_weight: f64,
    /// 可接受的最大成本（以此归一化）
    pub max_cost: u64,
    /// 风险阈值
    pub risk_thresholds: RiskThresholds,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            probability_weight: 0.5,
            cost_weight: 0.3,
            stability_weight: 0.2,
            max_cost: 10_000,
            risk_thresholds: RiskThresholds::default(),
        }
    }
}

/// 用于分类的风险阈值
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskThresholds {
    pub low: f64,
    pub medium: f64,
    pub high: f64,
}

impl Default for RiskThresholds {
    fn default() -> Self {
        Self {
            low: 0.8,
            medium: 0.6,
            high: 0.4,
        }
    }
}

/// 用于偏置概率的历史经验记录
#[derive(Debug, Clone)]
pub struct ExperienceRecord {
    pub path_id: String,
    pub success: bool,
    pub actual_cost: u64,
}

/// 使用蒙特卡洛仿真结果的概率规划器
pub struct ProbabilisticPlanner {
    config: PlannerConfig,
    /// 用于概率调整的历史经验
    experience: Vec<ExperienceRecord>,
}

impl ProbabilisticPlanner {
    pub fn new(config: PlannerConfig) -> Self {
        Self {
            config,
            experience: Vec::new(),
        }
    }

    /// With config and experiences.
    pub fn with_config_and_experiences(
        config: PlannerConfig,
        experience: Vec<ExperienceRecord>,
    ) -> Self {
        Self { config, experience }
    }

    /// 按综合得分对多条路径排序（越高越好）
    pub fn rank_paths(&self, results: &[SimulationResult]) -> Vec<RankedPath> {
        let mut ranked: Vec<RankedPath> =
            results.iter().map(|r| self.compute_ranked_path(r)).collect();

        ranked.sort_by(|a, b| {
            b.composite_score.partial_cmp(&a.composite_score).unwrap_or(std::cmp::Ordering::Equal)
        });

        ranked
    }

    /// 综合考虑成本/成功权衡选择最优路径
    pub fn select_optimal_path(&self, results: &[SimulationResult]) -> Option<RankedPath> {
        if results.is_empty() {
            return None;
        }
        let ranked = self.rank_paths(results);
        // 过滤掉严重风险路径，除非全部都是严重风险
        let safe_paths: Vec<_> =
            ranked.iter().filter(|p| p.risk_level != RiskLevel::Critical).cloned().collect();

        if safe_paths.is_empty() {
            // 全部路径都是严重风险时，返回其中最优者
            ranked.into_iter().next()
        } else {
            safe_paths.into_iter().next()
        }
    }

    /// 计算单条路径的风险分数（0.0 = 低风险，1.0 = 高风险）
    pub fn compute_risk_score(&self, result: &SimulationResult) -> f64 {
        // 风险 = 成功概率的倒数，按成本加权
        let prob_risk = 1.0 - result.success_probability;
        let cost_factor = (result.estimated_cost as f64 / self.config.max_cost as f64).min(1.0);
        let stability_risk = 1.0 - result.predicted_stability;

        // 加权组合
        (prob_risk * 0.5 + cost_factor * 0.3 + stability_risk * 0.2).clamp(0.0, 1.0)
    }

    /// 从风险分数分类风险等级
    pub fn classify_risk(&self, risk_score: f64) -> RiskLevel {
        let t = &self.config.risk_thresholds;
        if risk_score <= (1.0 - t.low) {
            RiskLevel::Low
        } else if risk_score <= (1.0 - t.medium) {
            RiskLevel::Medium
        } else if risk_score <= (1.0 - t.high) {
            RiskLevel::High
        } else {
            RiskLevel::Critical
        }
    }

    /// 基于历史经验调整成功概率
    pub fn adjust_probability_with_experience(&self, path_id: &str, base_probability: f64) -> f64 {
        let relevant: Vec<&ExperienceRecord> =
            self.experience.iter().filter(|e| e.path_id == path_id).collect();

        if relevant.is_empty() {
            return base_probability;
        }

        let success_count = relevant.iter().filter(|e| e.success).count() as f64;
        let total = relevant.len() as f64;
        let historical_rate = success_count / total;

        // 贝叶斯混合：60% 仿真 + 40% 历史
        base_probability * 0.6 + historical_rate * 0.4
    }

    /// 计算一组路径的汇总统计
    pub fn summarize(&self, results: &[SimulationResult]) -> PathSummary {
        if results.is_empty() {
            return PathSummary::default();
        }

        let ranked = self.rank_paths(results);
        let avg_prob: f64 =
            ranked.iter().map(|p| p.success_probability).sum::<f64>() / ranked.len() as f64;
        let avg_cost: u64 =
            ranked.iter().map(|p| p.estimated_cost).sum::<u64>() / ranked.len() as u64;
        let risk_distribution = {
            let mut dist = HashMap::new();
            for p in &ranked {
                *dist.entry(p.risk_level).or_insert(0) += 1;
            }
            dist
        };

        PathSummary {
            total_paths: ranked.len(),
            average_probability: avg_prob,
            average_cost: avg_cost,
            best_path: ranked.first().cloned(),
            worst_path: ranked.last().cloned(),
            risk_distribution,
        }
    }

    fn compute_ranked_path(&self, result: &SimulationResult) -> RankedPath {
        let adj_prob =
            self.adjust_probability_with_experience(&result.path_id, result.success_probability);
        let risk_score = self.compute_risk_score(result);
        let risk_level = self.classify_risk(risk_score);

        // 综合得分：越高越好
        let cost_norm = (result.estimated_cost as f64 / self.config.max_cost as f64).min(1.0);
        let composite = adj_prob * self.config.probability_weight
            + (1.0 - cost_norm) * self.config.cost_weight
            + result.predicted_stability * self.config.stability_weight;

        RankedPath {
            path_id: result.path_id.clone(),
            success_probability: adj_prob,
            estimated_cost: result.estimated_cost,
            risk_level,
            composite_score: composite,
            predicted_outcome: result.predicted_outcome.clone(),
            confidence: result.predicted_stability,
        }
    }
}

/// 路径分析汇总
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathSummary {
    pub total_paths: usize,
    pub average_probability: f64,
    pub average_cost: u64,
    pub best_path: Option<RankedPath>,
    pub worst_path: Option<RankedPath>,
    pub risk_distribution: HashMap<RiskLevel, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_results() -> Vec<SimulationResult> {
        vec![
            SimulationResult {
                path_id: "fast".to_string(),
                success_probability: 0.7,
                estimated_cost: 1000,
                predicted_stability: 0.8,
                predicted_effort: 500,
                predicted_outcome: "Quick completion".to_string(),
            },
            SimulationResult {
                path_id: "safe".to_string(),
                success_probability: 0.95,
                estimated_cost: 5000,
                predicted_stability: 0.95,
                predicted_effort: 2000,
                predicted_outcome: "Reliable completion".to_string(),
            },
            SimulationResult {
                path_id: "risky".to_string(),
                success_probability: 0.3,
                estimated_cost: 500,
                predicted_stability: 0.4,
                predicted_effort: 300,
                predicted_outcome: "Maybe works".to_string(),
            },
        ]
    }

    #[test]
    fn test_rank_paths() {
        let planner = ProbabilisticPlanner::new(PlannerConfig::default());
        let results = test_results();
        let ranked = planner.rank_paths(&results);
        assert_eq!(ranked.len(), 3);
        // 安全路径应因高概率排第一
        assert_eq!(ranked[0].path_id, "safe");
    }

    #[test]
    fn test_select_optimal() {
        let planner = ProbabilisticPlanner::new(PlannerConfig::default());
        let results = test_results();
        let optimal = planner.select_optimal_path(&results).unwrap();
        assert!(optimal.risk_level != RiskLevel::Critical);
    }

    #[test]
    fn test_risk_classification() {
        let planner = ProbabilisticPlanner::new(PlannerConfig::default());
        // classify_risk：得分 <= 0.2=低，<=0.4=中，<=0.6=高，其余为严重
        assert_eq!(planner.classify_risk(0.1), RiskLevel::Low);
        assert_eq!(planner.classify_risk(0.3), RiskLevel::Medium);
        assert_eq!(planner.classify_risk(0.5), RiskLevel::High);
        assert_eq!(planner.classify_risk(0.8), RiskLevel::Critical);
    }

    #[test]
    fn test_experience_adjustment() {
        let experiences = vec![
            ExperienceRecord {
                path_id: "fast".to_string(),
                success: true,
                actual_cost: 1200,
            },
            ExperienceRecord {
                path_id: "fast".to_string(),
                success: false,
                actual_cost: 2000,
            },
            ExperienceRecord {
                path_id: "fast".to_string(),
                success: true,
                actual_cost: 900,
            },
        ];
        let planner = ProbabilisticPlanner::with_config_and_experiences(
            PlannerConfig::default(),
            experiences,
        );
        let adjusted = planner.adjust_probability_with_experience("fast", 0.7);
        // 历史成功率 = 2/3 = 0.667
        // 混合：0.7 * 0.6 + 0.667 * 0.4 = 0.42 + 0.267 = 0.687
        assert!((adjusted - 0.687).abs() < 0.01);
    }
}
