use serde::{Deserialize, Serialize};
use sisyphus_tools_core::ToolResult;
use sisyphus_types::ModelRouteStrategy;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Configuration for a registered LLM model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub cost_per_1k_tokens: f64,
    pub provider: String,
    pub max_tokens: u32,
    pub endpoint: String,
    pub complexity_ceiling: u32, // max prompt complexity this model can handle
}

/// Tracks usage and budget for model routing decisions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageRecord {
    pub total_calls: u64,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub monthly_cost: f64,
    pub month_start: u64,
}

/// Cost-aware router that selects the best model for each request.
pub struct ModelRouter {
    models: Arc<RwLock<Vec<ModelConfig>>>,
    usage: Arc<RwLock<HashMap<String, UsageRecord>>>,
    monthly_budget: f64,
}

impl ModelRouter {
    /// Create a router with an optional monthly budget.
    /// budget=0 means unlimited.
    pub fn new(monthly_budget: f64) -> Self {
        Self {
            models: Arc::new(RwLock::new(Vec::new())),
            usage: Arc::new(RwLock::new(HashMap::new())),
            monthly_budget,
        }
    }

    /// Register a model for routing.
    pub fn register_model(&self, config: ModelConfig) {
        if let Ok(mut models) = self.models.write() {
            // Replace if exists, otherwise add
            if let Some(pos) = models.iter().position(|m| m.name == config.name) {
                models[pos] = config;
            } else {
                models.push(config);
            }
        }
    }

    /// Select the best model for a prompt given the strategy.
    pub fn select(&self, prompt: &str, strategy: &ModelRouteStrategy) -> ToolResult<ModelConfig> {
        let models = self.models.read().map_err(|e| {
            sisyphus_tools_core::ToolError::Generic(format!("ModelRouter lock poisoned: {e}"))
        })?;

        if models.is_empty() {
            return Err(sisyphus_tools_core::ToolError::Generic(
                "No models registered".into(),
            ));
        }

        let estimated_complexity = estimate_complexity(prompt);

        match strategy {
            ModelRouteStrategy::CostOptimized => {
                // Find cheapest model that can handle this complexity
                let mut sorted: Vec<&ModelConfig> = models
                    .iter()
                    .filter(|m| m.complexity_ceiling >= estimated_complexity)
                    .collect();
                sorted.sort_by(|a, b| {
                    a.cost_per_1k_tokens
                        .partial_cmp(&b.cost_per_1k_tokens)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                sorted.first().copied().cloned().ok_or_else(|| {
                    sisyphus_tools_core::ToolError::Generic(
                        "No model can handle this prompt complexity".into(),
                    )
                })
            },
            ModelRouteStrategy::PerformanceOptimized => {
                // Find most capable model (highest complexity ceiling)
                models.iter().max_by_key(|m| m.complexity_ceiling).cloned().ok_or_else(|| {
                    sisyphus_tools_core::ToolError::Generic("No models available".into())
                })
            },
            ModelRouteStrategy::FallbackChain => {
                // Try models in order of registration
                models.first().cloned().ok_or_else(|| {
                    sisyphus_tools_core::ToolError::Generic("No models available".into())
                })
            },
        }
    }

    /// Record usage for a model call.
    pub fn record_usage(&self, model: &str, cost: f64, tokens: u64) {
        if let Ok(mut usage) = self.usage.write() {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

            let entry = usage.entry(model.to_string()).or_insert_with(|| {
                let month_start = now - (now % (30 * 24 * 3600));
                UsageRecord {
                    month_start,
                    ..Default::default()
                }
            });

            // Reset monthly if a new month started
            let month_start = now - (now % (30 * 24 * 3600));
            if entry.month_start < month_start {
                entry.monthly_cost = 0.0;
                entry.month_start = month_start;
            }

            entry.total_calls += 1;
            entry.total_tokens += tokens;
            entry.total_cost += cost;
            entry.monthly_cost += cost;
        }
    }

    /// Check remaining budget for the current month.
    pub fn budget_remaining(&self) -> f64 {
        if self.monthly_budget <= 0.0 {
            return f64::INFINITY;
        }
        match self.usage.read() {
            Ok(usage) => {
                let total_monthly: f64 = usage.values().map(|u| u.monthly_cost).sum();
                (self.monthly_budget - total_monthly).max(0.0)
            },
            _ => 0.0,
        }
    }

    /// Get usage record for a model.
    pub fn get_usage(&self, model: &str) -> Option<UsageRecord> {
        self.usage.read().ok().and_then(|u| u.get(model).cloned())
    }

    /// List all registered models.
    pub fn list_models(&self) -> Vec<ModelConfig> {
        self.models.read().ok().map(|m| m.clone()).unwrap_or_default()
    }
}

/// Estimate prompt complexity based on length and structural indicators.
fn estimate_complexity(prompt: &str) -> u32 {
    let len = prompt.len() as u32;
    let has_code = prompt.contains("```") || prompt.contains("fn ") || prompt.contains("impl ");
    let has_math = prompt.contains("complexity")
        || prompt.contains("algorithm")
        || prompt.contains("O(")
        || prompt.contains("proof");
    let has_multi_lang = prompt.lines().count() > 20;

    let mut score = if len > 0 { (len / 10).max(1) } else { 0 };
    if has_code {
        score += 10;
    }
    if has_math {
        score += 15;
    }
    if has_multi_lang {
        score += 5;
    }
    score.min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_router() -> ModelRouter {
        let router = ModelRouter::new(100.0);
        router.register_model(ModelConfig {
            name: "gpt-4o".into(),
            cost_per_1k_tokens: 0.01,
            provider: "openai".into(),
            max_tokens: 128_000,
            endpoint: "https://api.openai.com/v1".into(),
            complexity_ceiling: 100,
        });
        router.register_model(ModelConfig {
            name: "gpt-4o-mini".into(),
            cost_per_1k_tokens: 0.001,
            provider: "openai".into(),
            max_tokens: 128_000,
            endpoint: "https://api.openai.com/v1".into(),
            complexity_ceiling: 50,
        });
        router.register_model(ModelConfig {
            name: "gemma-3".into(),
            cost_per_1k_tokens: 0.0005,
            provider: "google".into(),
            max_tokens: 32_000,
            endpoint: "http://localhost:8000/v1".into(),
            complexity_ceiling: 30,
        });
        router
    }

    #[test]
    fn test_cost_optimized_selects_cheapest() {
        let router = setup_router();
        let simple_prompt = "hello world";
        let model = router.select(simple_prompt, &ModelRouteStrategy::CostOptimized).unwrap();
        // gemma-3 is cheapest at 0.0005/1k and can handle 30 complexity
        assert_eq!(model.name, "gemma-3");
    }

    #[test]
    fn test_performance_selects_most_capable() {
        let router = setup_router();
        let model =
            router.select("complex stuff", &ModelRouteStrategy::PerformanceOptimized).unwrap();
        assert_eq!(model.name, "gpt-4o");
    }

    #[test]
    fn test_fallback_chain_returns_first() {
        let router = setup_router();
        let model = router.select("test", &ModelRouteStrategy::FallbackChain).unwrap();
        assert_eq!(model.name, "gpt-4o");
    }

    #[test]
    fn test_no_models_returns_error() {
        let router = ModelRouter::new(0.0);
        let result = router.select("test", &ModelRouteStrategy::CostOptimized);
        assert!(result.is_err());
    }

    #[test]
    fn test_budget_remaining() {
        let router = setup_router();
        assert_eq!(router.budget_remaining(), 100.0);

        router.record_usage("gpt-4o", 30.0, 1000);
        let remaining = router.budget_remaining();
        assert!((remaining - 70.0).abs() < 0.001);
    }

    #[test]
    fn test_register_replace_model() {
        let router = setup_router();
        assert_eq!(router.list_models().len(), 3);

        router.register_model(ModelConfig {
            name: "gpt-4o".into(),
            cost_per_1k_tokens: 0.02, // updated price
            provider: "openai".into(),
            max_tokens: 128_000,
            endpoint: "".into(),
            complexity_ceiling: 100,
        });
        assert_eq!(router.list_models().len(), 3); // replaced, not added
        let updated = router.list_models();
        let gpt4o = updated.iter().find(|m| m.name == "gpt-4o").unwrap();
        assert!((gpt4o.cost_per_1k_tokens - 0.02).abs() < 0.001);
    }

    #[test]
    fn test_estimate_complexity() {
        assert!(estimate_complexity("hi") > 0);
        assert!(
            estimate_complexity("```\nfn hello() {}\n```\nThis has complexity O(n log n)")
                > estimate_complexity("hi")
        );
    }
}
