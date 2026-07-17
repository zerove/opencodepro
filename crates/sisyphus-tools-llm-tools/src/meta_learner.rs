//! Meta-learning: learns optimal strategies from historical execution data.
//!
//! ## Sub-modules
//! - [`types`]: StrategyStats, LearningInsight, StrategyInsight.

pub mod types;

pub use types::{LearningInsight, StrategyInsight, StrategyStats};

use sisyphus_tools_core::{ToolError, ToolResult};
use sisyphus_types::MetaLearningSample;
use std::collections::HashMap;
use std::path::PathBuf;

/// Learns optimal strategies from historical execution data.
pub struct MetaLearner {
    storage_path: PathBuf,
    samples: Vec<MetaLearningSample>,
}

impl MetaLearner {
    /// Create a meta-learner with storage at the given path.
    pub fn new(storage_dir: &str) -> Self {
        let storage_path = PathBuf::from(storage_dir);
        let samples = Self::load_samples(&storage_path);
        Self {
            storage_path,
            samples,
        }
    }

    /// Create with default path .omo/meta/
    pub fn default_dir() -> Self {
        Self::new(".omo/meta")
    }

    /// Record a new execution outcome for learning.
    pub fn record_outcome(&mut self, sample: MetaLearningSample) -> ToolResult<()> {
        self.samples.push(sample);
        self.save_samples()
    }

    /// Calculate similarity between two context values.
    fn calculate_similarity(ctx1: &serde_json::Value, ctx2: &serde_json::Value) -> f64 {
        if let (Some(obj1), Some(obj2)) = (ctx1.as_object(), ctx2.as_object()) {
            let mut score = 0.0;
            let mut total_weight = 0.0;

            for (key, val1) in obj1 {
                let weight = match key.as_str() {
                    "goal" | "goal_type" => 2.0,
                    "node_count" | "estimated_cost" => 1.0,
                    _ => 0.5,
                };
                total_weight += weight;

                if let Some(val2) = obj2.get(key) {
                    if val1 == val2 {
                        score += weight;
                    } else if let (Some(n1), Some(n2)) = (val1.as_f64(), val2.as_f64()) {
                        // Numeric bucketing for counts/costs
                        let diff = (n1 - n2).abs();
                        let range = n1.max(n2) * 0.2; // 20% tolerance
                        if diff <= range {
                            score += weight * 0.7;
                        }
                    } else if let (Some(s1), Some(s2)) = (val1.as_str(), val2.as_str()) {
                        // Simple keyword overlap for strings
                        let s1_set: std::collections::HashSet<_> = s1.split_whitespace().collect();
                        let s2_set: std::collections::HashSet<_> = s2.split_whitespace().collect();
                        let intersection = s1_set.intersection(&s2_set).count();
                        let union = s1_set.union(&s2_set).count();
                        if union > 0 {
                            score += weight * (intersection as f64 / union as f64);
                        }
                    }
                }
            }
            if total_weight == 0.0 { 0.0 } else { score / total_weight }
        } else {
            if ctx1 == ctx2 { 1.0 } else { 0.0 }
        }
    }

    /// Find the best strategy and its parameters for a given context.
    pub fn best_strategy_params(&self, context: &serde_json::Value) -> Option<(String, serde_json::Value)> {
        // Group samples by strategy and calculate weighted success
        let mut strategy_scores: HashMap<String, (f64, Vec<&MetaLearningSample>)> = HashMap::new();

        for sample in &self.samples {
            let similarity = Self::calculate_similarity(context, &sample.context);
            if similarity <= 0.0 {
                continue;
            }

            let entry = strategy_scores.entry(sample.strategy.clone()).or_insert((0.0, Vec::new()));
            entry.1.push(sample);
            
            // Weighted success: similarity * (1 if success else 0)
            if sample.success {
                entry.0 += similarity;
            }
        }

        // Find strategy with highest weighted success
        let mut best: Option<(String, f64, serde_json::Value)> = None;
        for (strategy, (weighted_success, samples)) in &strategy_scores {
            if samples.is_empty() {
                continue;
            }
            
            // Normalize success rate by total similarity of samples in this strategy
            let total_sim: f64 = samples.iter()
                .map(|s| Self::calculate_similarity(context, &s.context))
                .sum();
            let normalized_score = if total_sim > 0.0 { weighted_success / total_sim } else { 0.0 };

            if normalized_score >= 0.5 {
                match best {
                    Some((_, best_score, _)) if normalized_score > best_score => {
                        let params = samples.iter()
                            .rfind(|s| s.success)
                            .map(|s| s.params.clone())
                            .unwrap_or_else(|| serde_json::json!({}));
                        best = Some((strategy.clone(), normalized_score, params));
                    }
                    None => {
                        let params = samples.iter()
                            .rfind(|s| s.success)
                            .map(|s| s.params.clone())
                            .unwrap_or_else(|| serde_json::json!({}));
                        best = Some((strategy.clone(), normalized_score, params));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(s, _, p)| (s, p))
    }

    /// Get statistics for a specific strategy.
    pub fn strategy_stats(&self, strategy: &str) -> StrategyStats {
        let relevant: Vec<&MetaLearningSample> = self
            .samples
            .iter()
            .filter(|s| s.strategy == strategy)
            .collect();

        let total = relevant.len() as u64;
        if total == 0 {
            return StrategyStats::default();
        }

        let successes = relevant.iter().filter(|s| s.success).count() as u64;
        let failures = total - successes;
        let total_duration: f64 = relevant.iter().map(|s| s.duration_secs).sum();
        let avg_duration = total_duration / total as f64;

        StrategyStats {
            total_uses: total,
            successes,
            failures,
            avg_duration,
            success_rate: successes as f64 / total as f64,
        }
    }

    /// Get all stored samples.
    pub fn all_samples(&self) -> &[MetaLearningSample] {
        &self.samples
    }

    /// List all strategies that have been used.
    pub fn list_strategies(&self) -> Vec<String> {
        let mut strategies: Vec<String> = self.samples.iter().map(|s| s.strategy.clone()).collect();
        strategies.sort();
        strategies.dedup();
        strategies
    }

    /// Analyze all samples to derive learning insights.
    pub fn learn(&self) -> LearningInsight {
        let total_samples = self.samples.len() as u64;
        if total_samples == 0 {
            return LearningInsight {
                total_samples: 0,
                overall_success_rate: 0.0,
                strategies: Vec::new(),
                trend: "stable".into(),
            };
        }

        let total_successes = self.samples.iter().filter(|s| s.success).count() as u64;
        let overall_success_rate = total_successes as f64 / total_samples as f64;

        // Group by strategy
        let mut strategy_map: HashMap<String, Vec<&MetaLearningSample>> = HashMap::new();
        for sample in &self.samples {
            strategy_map
                .entry(sample.strategy.clone())
                .or_default()
                .push(sample);
        }

        let mut strategies = Vec::new();
        for (strategy, samples) in strategy_map {
            let total_uses = samples.len() as u64;
            let successes = samples.iter().filter(|s| s.success).count() as u64;
            let failures = total_uses - successes;
            let total_duration: f64 = samples.iter().map(|s| s.duration_secs).sum();
            let avg_duration_secs = total_duration / total_uses as f64;
            let success_rate = successes as f64 / total_uses as f64;

            strategies.push(StrategyInsight {
                strategy,
                total_uses,
                successes,
                failures,
                avg_duration_secs,
                success_rate,
            });
        }

        // Determine trend: compare first half vs second half
        let trend = if total_samples < 2 {
            "stable".into()
        } else {
            let mid = self.samples.len() / 2;
            let first_half = &self.samples[..mid];
            let second_half = &self.samples[mid..];

            let first_rate =
                first_half.iter().filter(|s| s.success).count() as f64 / first_half.len() as f64;
            let second_rate =
                second_half.iter().filter(|s| s.success).count() as f64 / second_half.len() as f64;

            if second_rate > first_rate + 0.05 {
                "improving".into()
            } else if second_rate < first_rate - 0.05 {
                "degrading".into()
            } else {
                "stable".into()
            }
        };

        LearningInsight {
            total_samples,
            overall_success_rate,
            strategies,
            trend,
        }
    }

    /// Clear all stored samples.
    pub fn clear(&mut self) -> ToolResult<()> {
        self.samples.clear();
        self.save_samples()
    }

    fn load_samples(path: &PathBuf) -> Vec<MetaLearningSample> {
        std::fs::create_dir_all(path).ok();
        let file_path = path.join("samples.json");
        std::fs::read_to_string(&file_path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn save_samples(&self) -> ToolResult<()> {
        std::fs::create_dir_all(&self.storage_path).map_err(ToolError::Io)?;
        let file_path = self.storage_path.join("samples.json");
        let content =
            serde_json::to_string_pretty(&self.samples).map_err(|e| ToolError::Generic(e.to_string()))?;
        std::fs::write(&file_path, content).map_err(ToolError::Io)
    }

    /// Save the latest learning insight to disk.
    pub fn save_insight(&self, insight: &LearningInsight) -> ToolResult<()> {
        std::fs::create_dir_all(&self.storage_path).map_err(ToolError::Io)?;
        let file_path = self.storage_path.join("insight.json");
        let content =
            serde_json::to_string_pretty(insight).map_err(|e| ToolError::Generic(e.to_string()))?;
        std::fs::write(&file_path, content).map_err(ToolError::Io)
    }

    /// Load the latest learning insight from disk.
    pub fn load_latest_insight(&self) -> Option<LearningInsight> {
        let file_path = self.storage_path.join("insight.json");
        std::fs::read_to_string(&file_path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample(strategy: &str, success: bool, ctx: serde_json::Value) -> MetaLearningSample {
        MetaLearningSample {
            context: ctx,
            strategy: strategy.into(),
            success,
            duration_secs: 1.0,
            timestamp: 0,
        }
    }

    #[test]
    fn test_record_and_list_strategies() {
        let dir = tempfile::tempdir().unwrap();
        let mut ml = MetaLearner::new(dir.path().to_str().unwrap());

        ml.record_outcome(sample("fast", true, json!({"lang": "rust"})))
            .unwrap();
        ml.record_outcome(sample("accurate", false, json!({"lang": "rust"})))
            .unwrap();

        let strategies = ml.list_strategies();
        assert!(strategies.contains(&"fast".to_string()));
        assert!(strategies.contains(&"accurate".to_string()));
    }

    #[test]
    fn test_best_strategy() {
        let dir = tempfile::tempdir().unwrap();
        let mut ml = MetaLearner::new(dir.path().to_str().unwrap());

        for _ in 0..3 {
            ml.record_outcome(sample("fast", true, json!({"lang": "rust"})))
                .unwrap();
        }
        ml.record_outcome(sample("slow", true, json!({"lang": "rust"})))
            .unwrap();
        ml.record_outcome(sample("slow", false, json!({"lang": "rust"})))
            .unwrap();
        ml.record_outcome(sample("slow", false, json!({"lang": "rust"})))
            .unwrap();

        let best = ml.best_strategy(&json!({"lang": "rust"}));
        assert_eq!(best, Some("fast".to_string()));
    }

    #[test]
    fn test_best_strategy_insufficient_data() {
        let dir = tempfile::tempdir().unwrap();
        let ml = MetaLearner::new(dir.path().to_str().unwrap());
        assert!(ml.best_strategy(&json!({})).is_none());
    }

    #[test]
    fn test_strategy_stats() {
        let dir = tempfile::tempdir().unwrap();
        let mut ml = MetaLearner::new(dir.path().to_str().unwrap());

        ml.record_outcome(sample("test_s", true, json!({}))).unwrap();
        ml.record_outcome(sample("test_s", true, json!({}))).unwrap();
        ml.record_outcome(sample("test_s", false, json!({}))).unwrap();

        let stats = ml.strategy_stats("test_s");
        assert_eq!(stats.total_uses, 3);
        assert_eq!(stats.successes, 2);
        assert_eq!(stats.failures, 1);
        assert!((stats.success_rate - 2.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn test_strategy_stats_nonexistent() {
        let ml = MetaLearner::default_dir();
        let stats = ml.strategy_stats("nonexistent");
        assert_eq!(stats.total_uses, 0);
        assert_eq!(stats.success_rate, 0.0);
    }

    #[test]
    fn test_clear() {
        let dir = tempfile::tempdir().unwrap();
        let mut ml = MetaLearner::new(dir.path().to_str().unwrap());

        ml.record_outcome(sample("s1", true, json!({}))).unwrap();
        assert_eq!(ml.all_samples().len(), 1);

        ml.clear().unwrap();
        assert_eq!(ml.all_samples().len(), 0);
    }

    #[test]
    fn test_learn_empty() {
        let ml = MetaLearner::default_dir();
        let insight = ml.learn();
        assert_eq!(insight.total_samples, 0);
        assert_eq!(insight.overall_success_rate, 0.0);
        assert!(insight.strategies.is_empty());
        assert_eq!(insight.trend, "stable");
    }

    #[test]
    fn test_learn_with_data() {
        let dir = tempfile::tempdir().unwrap();
        let mut ml = MetaLearner::new(dir.path().to_str().unwrap());

        for _ in 0..3 {
            ml.record_outcome(sample("fast", true, json!({"lang": "rust"})))
                .unwrap();
        }
        ml.record_outcome(sample("slow", true, json!({"lang": "rust"})))
            .unwrap();
        ml.record_outcome(sample("slow", false, json!({"lang": "rust"})))
            .unwrap();
        ml.record_outcome(sample("slow", false, json!({"lang": "rust"})))
            .unwrap();

        let insight = ml.learn();
        assert_eq!(insight.total_samples, 6);
        assert!((insight.overall_success_rate - 4.0 / 6.0).abs() < 0.001);
        assert_eq!(insight.strategies.len(), 2);

        let fast = insight
            .strategies
            .iter()
            .find(|s| s.strategy == "fast")
            .unwrap();
        assert_eq!(fast.total_uses, 3);
        assert_eq!(fast.successes, 3);
        assert_eq!(fast.failures, 0);
        assert!((fast.success_rate - 1.0).abs() < 0.001);

        let slow = insight
            .strategies
            .iter()
            .find(|s| s.strategy == "slow")
            .unwrap();
        assert_eq!(slow.total_uses, 3);
        assert_eq!(slow.successes, 1);
        assert_eq!(slow.failures, 2);
        assert!((slow.success_rate - 1.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn test_learn_trend_improving() {
        let dir = tempfile::tempdir().unwrap();
        let mut ml = MetaLearner::new(dir.path().to_str().unwrap());

        for _ in 0..3 {
            ml.record_outcome(sample("s1", false, json!({}))).unwrap();
        }
        for _ in 0..3 {
            ml.record_outcome(sample("s1", true, json!({}))).unwrap();
        }

        let insight = ml.learn();
        assert_eq!(insight.trend, "improving");
    }

    #[test]
    fn test_learn_trend_degrading() {
        let dir = tempfile::tempdir().unwrap();
        let mut ml = MetaLearner::new(dir.path().to_str().unwrap());

        for _ in 0..3 {
            ml.record_outcome(sample("s1", true, json!({}))).unwrap();
        }
        for _ in 0..3 {
            ml.record_outcome(sample("s1", false, json!({}))).unwrap();
        }

        let insight = ml.learn();
        assert_eq!(insight.trend, "degrading");
    }

    #[test]
    fn test_learn_trend_stable() {
        let dir = tempfile::tempdir().unwrap();
        let mut ml = MetaLearner::new(dir.path().to_str().unwrap());

        for _ in 0..6 {
            ml.record_outcome(sample("s1", true, json!({}))).unwrap();
        }

        let insight = ml.learn();
        assert_eq!(insight.trend, "stable");
    }

    #[test]
    fn test_persistence_across_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_string();

        {
            let mut ml = MetaLearner::new(&path);
            ml.record_outcome(sample("persist", true, json!({})))
                .unwrap();
        }

        let ml = MetaLearner::new(&path);
        assert_eq!(ml.all_samples().len(), 1);
        assert_eq!(ml.all_samples()[0].strategy, "persist");
    }
}
