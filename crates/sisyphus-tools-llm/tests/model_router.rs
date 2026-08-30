//! Integration tests for sisyphus-tools-llm `model_router` module.
//!
//! Exercises the public surfaces of [`ModelConfig`], [`UsageRecord`],
//! and [`ModelRouter`]: registration, selection strategies, usage
//! tracking, and budget accounting.

use sisyphus_tools_llm::model_router::{ModelConfig, ModelRouter, UsageRecord};
use sisyphus_types::ModelRouteStrategy;

fn cfg(name: &str, cost: f64, complexity: u32) -> ModelConfig {
    ModelConfig {
        name: name.to_string(),
        cost_per_1k_tokens: cost,
        provider: "openai".to_string(),
        max_tokens: 4096,
        endpoint: "https://api.openai.com/v1".to_string(),
        complexity_ceiling: complexity,
    }
}

// ---------------------------------------------------------------------------
// ModelConfig
// ---------------------------------------------------------------------------

#[test]
fn model_config_field_assignment() {
    let c = cfg("gpt-4o", 0.005, 100);
    assert_eq!(c.name, "gpt-4o");
    assert!((c.cost_per_1k_tokens - 0.005).abs() < 1e-9);
    assert_eq!(c.complexity_ceiling, 100);
}

#[test]
fn model_config_serde() {
    let c = cfg("gpt-4o", 0.005, 100);
    let j = serde_json::to_string(&c).unwrap();
    let back: ModelConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "gpt-4o");
    assert_eq!(back.max_tokens, 4096);
}

// ---------------------------------------------------------------------------
// ModelRouter
// ---------------------------------------------------------------------------

#[test]
fn router_new_has_no_models() {
    let r = ModelRouter::new(100.0);
    assert!(r.list_models().is_empty());
}

#[test]
fn router_register_model() {
    let r = ModelRouter::new(100.0);
    r.register_model(cfg("m1", 0.001, 50));
    let list = r.list_models();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "m1");
}

#[test]
fn router_register_multiple_models() {
    let r = ModelRouter::new(100.0);
    r.register_model(cfg("cheap", 0.001, 10));
    r.register_model(cfg("expensive", 0.05, 100));
    assert_eq!(r.list_models().len(), 2);
}

#[test]
fn router_register_overwrites() {
    let r = ModelRouter::new(100.0);
    r.register_model(cfg("m1", 0.001, 10));
    r.register_model(cfg("m1", 0.999, 200));
    let list = r.list_models();
    assert_eq!(list.len(), 1);
    // The new cost should win.
    assert!((list[0].cost_per_1k_tokens - 0.999).abs() < 1e-9);
}

#[test]
fn router_select_cost_optimized() {
    let r = ModelRouter::new(100.0);
    r.register_model(cfg("cheap", 0.001, 10));
    r.register_model(cfg("expensive", 0.05, 10));
    let prompt = "short prompt here";
    let m = r.select(prompt, &ModelRouteStrategy::CostOptimized).unwrap();
    assert_eq!(m.name, "cheap");
}

#[test]
fn router_select_performance_optimized() {
    let r = ModelRouter::new(100.0);
    r.register_model(cfg("weak", 0.001, 5));
    r.register_model(cfg("strong", 0.05, 100));
    let m = r
        .select("complex prompt requiring more capacity", &ModelRouteStrategy::PerformanceOptimized)
        .unwrap();
    // Most capable (highest complexity_ceiling) wins.
    assert_eq!(m.name, "strong");
}

#[test]
fn router_select_no_models_errors() {
    let r = ModelRouter::new(100.0);
    let res = r.select("anything", &ModelRouteStrategy::CostOptimized);
    assert!(res.is_err());
}

#[test]
fn router_record_usage_and_get() {
    let r = ModelRouter::new(100.0);
    r.register_model(cfg("m1", 0.001, 10));
    r.record_usage("m1", 0.5, 1000);
    let usage = r.get_usage("m1").unwrap();
    assert_eq!(usage.total_tokens, 1000);
    assert!((usage.total_cost - 0.5).abs() < 1e-9);
    assert_eq!(usage.total_calls, 1);
}

#[test]
fn router_record_usage_unknown_model() {
    let r = ModelRouter::new(100.0);
    // record_usage auto-creates an entry even for unknown models.
    r.record_usage("nonexistent", 0.0, 0);
    let usage = r.get_usage("nonexistent").unwrap();
    assert_eq!(usage.total_calls, 1);
}

#[test]
fn router_budget_remaining_decreases() {
    let r = ModelRouter::new(10.0);
    r.register_model(cfg("m1", 1.0, 10));
    let initial = r.budget_remaining();
    assert!((initial - 10.0).abs() < 1e-9);
    r.record_usage("m1", 3.0, 100);
    let after = r.budget_remaining();
    assert!(after < initial);
}

#[test]
fn router_get_usage_missing_returns_none() {
    let r = ModelRouter::new(100.0);
    assert!(r.get_usage("never_registered").is_none());
}

// ---------------------------------------------------------------------------
// UsageRecord
// ---------------------------------------------------------------------------

#[test]
fn usage_record_field_assignment() {
    let u = UsageRecord {
        total_calls: 5,
        total_tokens: 2000,
        total_cost: 1.5,
        monthly_cost: 1.5,
        month_start: 12345,
    };
    assert_eq!(u.total_calls, 5);
    assert_eq!(u.total_tokens, 2000);
}

// ---------------------------------------------------------------------------
// ModelRouteStrategy
// ---------------------------------------------------------------------------

#[test]
fn model_route_strategy_variants_distinct() {
    assert_ne!(
        ModelRouteStrategy::CostOptimized,
        ModelRouteStrategy::PerformanceOptimized
    );
    assert_ne!(
        ModelRouteStrategy::PerformanceOptimized,
        ModelRouteStrategy::FallbackChain
    );
}

#[test]
fn model_route_strategy_serde() {
    for s in [
        ModelRouteStrategy::CostOptimized,
        ModelRouteStrategy::PerformanceOptimized,
        ModelRouteStrategy::FallbackChain,
    ] {
        let j = serde_json::to_string(&s).unwrap();
        let back: ModelRouteStrategy = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn model_config_unicode_name() {
    use sisyphus_tools_llm::model_router::ModelConfig;
    let c = ModelConfig {
        name: "模型-α".into(),
        cost_per_1k_tokens: 0.001,
        provider: "openai".into(),
        max_tokens: 8192,
        endpoint: "https://api.example.com".into(),
        complexity_ceiling: 100,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ModelConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "模型-α");
}

#[test]
fn model_config_zero_cost() {
    use sisyphus_tools_llm::model_router::ModelConfig;
    let c = ModelConfig {
        name: "free".into(),
        cost_per_1k_tokens: 0.0,
        provider: "p".into(),
        max_tokens: 0,
        endpoint: "".into(),
        complexity_ceiling: 0,
    };
    assert_eq!(c.cost_per_1k_tokens, 0.0);
    let j = serde_json::to_string(&c).unwrap();
    let back: ModelConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.max_tokens, 0);
}

#[test]
fn model_config_clone() {
    use sisyphus_tools_llm::model_router::ModelConfig;
    let c = ModelConfig {
        name: "x".into(),
        cost_per_1k_tokens: 1.0,
        provider: "p".into(),
        max_tokens: 100,
        endpoint: "e".into(),
        complexity_ceiling: 10,
    };
    let cloned = c.clone();
    assert_eq!(cloned.name, "x");
    assert_eq!(cloned.max_tokens, 100);
}

#[test]
fn usage_record_default() {
    use sisyphus_tools_llm::model_router::UsageRecord;
    let r = UsageRecord::default();
    assert_eq!(r.total_calls, 0);
    assert_eq!(r.total_tokens, 0);
    assert_eq!(r.total_cost, 0.0);
    assert_eq!(r.monthly_cost, 0.0);
    assert_eq!(r.month_start, 0);
}

#[test]
fn usage_record_serde() {
    use sisyphus_tools_llm::model_router::UsageRecord;
    let r = UsageRecord {
        total_calls: 100,
        total_tokens: 5000,
        total_cost: 0.5,
        monthly_cost: 5.0,
        month_start: 1700000000,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: UsageRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.total_calls, 100);
    assert_eq!(back.total_tokens, 5000);
    assert_eq!(back.month_start, 1700000000);
}

#[test]
fn usage_record_clone() {
    use sisyphus_tools_llm::model_router::UsageRecord;
    let r = UsageRecord {
        total_calls: 10,
        total_tokens: 100,
        total_cost: 0.1,
        monthly_cost: 1.0,
        month_start: 1,
    };
    let cloned = r.clone();
    assert_eq!(cloned.total_calls, 10);
}

#[test]
fn model_route_strategy_debug() {
    let _ = format!("{:?}", sisyphus_types::ModelRouteStrategy::CostOptimized);
    let _ = format!("{:?}", sisyphus_types::ModelRouteStrategy::PerformanceOptimized);
    let _ = format!("{:?}", sisyphus_types::ModelRouteStrategy::FallbackChain);
}

#[test]
fn model_route_strategy_copy() {
    let a = sisyphus_types::ModelRouteStrategy::CostOptimized;
    let b = a; // Copy
    assert_eq!(a, b);
}

#[test]
fn model_route_strategy_equality() {
    assert_eq!(
        sisyphus_types::ModelRouteStrategy::CostOptimized,
        sisyphus_types::ModelRouteStrategy::CostOptimized
    );
    assert_ne!(
        sisyphus_types::ModelRouteStrategy::CostOptimized,
        sisyphus_types::ModelRouteStrategy::PerformanceOptimized
    );
}
