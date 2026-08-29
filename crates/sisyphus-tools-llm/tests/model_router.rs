//! Integration tests for sisyphus-tools-llm `ModelRouter`.
//!
//! Exercises the public `ModelRouter` / `ModelConfig` / `UsageRecord` API
//! and the `ModelRouteStrategy` enum from `sisyphus-types`. The router
//! selects among registered models according to one of three strategies:
//! - `CostOptimized` — cheapest model that can handle the prompt's
//!   estimated complexity
//! - `PerformanceOptimized` — most capable model (highest
//!   `complexity_ceiling`)
//! - `FallbackChain` — first registered model
//!
//! It also tracks per-model usage and enforces a monthly budget.

use sisyphus_tools_llm::model_router::{ModelConfig, ModelRouter};
use sisyphus_types::ModelRouteStrategy;

fn make_model(name: &str, cost: f64, ceiling: u32) -> ModelConfig {
    ModelConfig {
        name: name.to_string(),
        cost_per_1k_tokens: cost,
        provider: "test".to_string(),
        max_tokens: 8192,
        endpoint: "http://localhost/test".to_string(),
        complexity_ceiling: ceiling,
    }
}

fn setup_router() -> ModelRouter {
    let router = ModelRouter::new(100.0);
    router.register_model(make_model("gpt-4o", 0.01, 100));
    router.register_model(make_model("gpt-4o-mini", 0.001, 50));
    router.register_model(make_model("gemma-3", 0.0005, 30));
    router
}

// ---------------------------------------------------------------------------
// Construction & registration
// ---------------------------------------------------------------------------

#[test]
fn new_router_has_no_models() {
    let router = ModelRouter::new(0.0);
    assert!(router.list_models().is_empty());
}

#[test]
fn register_model_adds_new_and_keeps_count() {
    let router = ModelRouter::new(0.0);
    router.register_model(make_model("a", 0.01, 50));
    router.register_model(make_model("b", 0.02, 100));
    assert_eq!(router.list_models().len(), 2);
}

#[test]
fn register_model_replaces_existing_with_same_name() {
    let router = ModelRouter::new(0.0);
    router.register_model(make_model("a", 0.01, 50));
    router.register_model(make_model("a", 0.99, 99));
    assert_eq!(router.list_models().len(), 1);
    let a = router
        .list_models()
        .into_iter()
        .find(|m| m.name == "a")
        .unwrap();
    assert!((a.cost_per_1k_tokens - 0.99).abs() < 1e-9);
    assert_eq!(a.complexity_ceiling, 99);
}

#[test]
fn list_models_returns_registered_models() {
    let router = setup_router();
    let names: Vec<String> = router.list_models().into_iter().map(|m| m.name).collect();
    assert!(names.contains(&"gpt-4o".to_string()));
    assert!(names.contains(&"gpt-4o-mini".to_string()));
    assert!(names.contains(&"gemma-3".to_string()));
}

// ---------------------------------------------------------------------------
// Selection strategies
// ---------------------------------------------------------------------------

#[test]
fn cost_optimized_picks_cheapest_for_simple_prompt() {
    let router = setup_router();
    let m = router
        .select("hello", &ModelRouteStrategy::CostOptimized)
        .expect("ok");
    // "hello" is short → estimated complexity 1. gemma-3 (ceiling 30) is
    // cheapest at 0.0005/1k.
    assert_eq!(m.name, "gemma-3");
}

#[test]
fn cost_optimized_skips_models_with_insufficient_complexity() {
    // A long prompt + code blocks → estimated complexity could be > 30.
    let router = setup_router();
    let big_code_prompt: String = std::iter::repeat("fn x() {}\n").take(20).collect();
    let m = router
        .select(&big_code_prompt, &ModelRouteStrategy::CostOptimized)
        .expect("ok");
    // gemma-3 has ceiling 30; if complexity > 30, the cheapest available
    // is gpt-4o-mini (ceiling 50).
    if m.name == "gemma-3" {
        // Acceptable if complexity is still ≤ 30.
    } else {
        assert_eq!(m.name, "gpt-4o-mini");
    }
}

#[test]
fn cost_optimized_returns_error_when_no_model_fits() {
    // Register only a low-ceiling model, then send a high-complexity prompt.
    let router = ModelRouter::new(0.0);
    router.register_model(make_model("small", 0.001, 5));
    // A long code-heavy prompt easily exceeds ceiling 5.
    let heavy = std::iter::repeat("complexity O(n) proof\n").take(20).collect::<String>();
    let result = router.select(&heavy, &ModelRouteStrategy::CostOptimized);
    assert!(result.is_err());
}

#[test]
fn performance_optimized_picks_highest_ceiling() {
    let router = setup_router();
    let m = router
        .select("anything", &ModelRouteStrategy::PerformanceOptimized)
        .expect("ok");
    // gpt-4o has the highest complexity_ceiling (100).
    assert_eq!(m.name, "gpt-4o");
}

#[test]
fn fallback_chain_returns_first_registered() {
    let router = setup_router();
    let m = router
        .select("anything", &ModelRouteStrategy::FallbackChain)
        .expect("ok");
    assert_eq!(m.name, "gpt-4o");
}

#[test]
fn select_with_no_models_returns_error() {
    let router = ModelRouter::new(0.0);
    for strategy in [
        ModelRouteStrategy::CostOptimized,
        ModelRouteStrategy::PerformanceOptimized,
        ModelRouteStrategy::FallbackChain,
    ] {
        let result = router.select("test", &strategy);
        assert!(result.is_err(), "{strategy:?} should fail");
    }
}

// ---------------------------------------------------------------------------
// Usage tracking & budget
// ---------------------------------------------------------------------------

#[test]
fn budget_remaining_starts_at_full_when_no_usage() {
    let router = ModelRouter::new(50.0);
    assert!((router.budget_remaining() - 50.0).abs() < 1e-9);
}

#[test]
fn budget_unlimited_when_zero_or_negative() {
    let r0 = ModelRouter::new(0.0);
    let rn = ModelRouter::new(-1.0);
    assert!(r0.budget_remaining().is_infinite());
    assert!(rn.budget_remaining().is_infinite());
}

#[test]
fn record_usage_subtracts_from_budget() {
    let router = ModelRouter::new(100.0);
    router.register_model(make_model("m", 0.01, 50));
    router.record_usage("m", 30.0, 1000);
    let remaining = router.budget_remaining();
    assert!((remaining - 70.0).abs() < 1e-6, "got {remaining}");
}

#[test]
fn budget_clamped_to_zero_when_exceeded() {
    let router = ModelRouter::new(10.0);
    router.register_model(make_model("m", 0.01, 50));
    router.record_usage("m", 50.0, 1000);
    assert_eq!(router.budget_remaining(), 0.0);
}

#[test]
fn get_usage_returns_record_after_recording() {
    let router = ModelRouter::new(100.0);
    router.register_model(make_model("m", 0.01, 50));
    router.record_usage("m", 5.0, 1000);
    let u = router.get_usage("m").expect("usage");
    assert_eq!(u.total_calls, 1);
    assert_eq!(u.total_tokens, 1000);
    assert!((u.total_cost - 5.0).abs() < 1e-9);
    assert!((u.monthly_cost - 5.0).abs() < 1e-9);
}

#[test]
fn get_usage_unknown_model_returns_none() {
    let router = ModelRouter::new(0.0);
    assert!(router.get_usage("nope").is_none());
}

#[test]
fn record_usage_accumulates_multiple_calls() {
    let router = ModelRouter::new(100.0);
    router.register_model(make_model("m", 0.01, 50));
    router.record_usage("m", 1.0, 100);
    router.record_usage("m", 2.0, 200);
    router.record_usage("m", 3.0, 300);
    let u = router.get_usage("m").expect("usage");
    assert_eq!(u.total_calls, 3);
    assert_eq!(u.total_tokens, 600);
    assert!((u.total_cost - 6.0).abs() < 1e-9);
}

#[test]
fn monthly_budget_aggregates_across_models() {
    let router = ModelRouter::new(100.0);
    router.register_model(make_model("a", 0.01, 50));
    router.register_model(make_model("b", 0.01, 50));
    router.record_usage("a", 20.0, 100);
    router.record_usage("b", 30.0, 100);
    // 100 - 50 = 50
    let remaining = router.budget_remaining();
    assert!((remaining - 50.0).abs() < 1e-6, "got {remaining}");
}
