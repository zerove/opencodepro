//! Shared types for the MetaLearner module.

use serde::{Deserialize, Serialize};

/// Statistics about a strategy's performance.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StrategyStats {
    pub total_uses: u64,
    pub successes: u64,
    pub failures: u64,
    pub avg_duration: f64,
    pub success_rate: f64,
}

/// Insight derived from analyzing historical learning samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningInsight {
    pub total_samples: u64,
    pub overall_success_rate: f64,
    pub strategies: Vec<StrategyInsight>,
    pub trend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyInsight {
    pub strategy: String,
    pub total_uses: u64,
    pub successes: u64,
    pub failures: u64,
    pub avg_duration_secs: f64,
    pub success_rate: f64,
}
