//! Integration tests for sisyphus-types `config::llm` module's
//! pure data types and constructors: `AgentLlmOverride`, `CategoryConfig`,
//! `BudgetMode`, `EmbeddingConfig`, `LlmConfig`, `default_llm_config`.

use sisyphus_types::config::llm::{
    default_llm_config, AgentLlmOverride, BudgetMode, CategoryConfig, EmbeddingConfig, LlmConfig,
};
use sisyphus_types::types::ModelRouteStrategy;

// ---------------------------------------------------------------------------
// AgentLlmOverride
// ---------------------------------------------------------------------------

#[test]
fn agent_llm_override_default() {
    let a = AgentLlmOverride::default();
    assert_eq!(a.model, "gpt-4o");
    assert!(a.endpoint.is_none());
    assert!(a.api_key.is_none());
}

#[test]
fn agent_llm_override_field_access() {
    let a = AgentLlmOverride {
        model: "claude-3".into(),
        endpoint: Some("https://api.anthropic.com".into()),
        api_key: Some("sk-test".into()),
    };
    assert_eq!(a.model, "claude-3");
    assert_eq!(a.endpoint.as_deref(), Some("https://api.anthropic.com"));
    assert_eq!(a.api_key.as_deref(), Some("sk-test"));
}

#[test]
fn agent_llm_override_serde() {
    let a = AgentLlmOverride {
        model: "x".into(),
        endpoint: None,
        api_key: None,
    };
    let j = serde_json::to_string(&a).unwrap();
    let back: AgentLlmOverride = serde_json::from_str(&j).unwrap();
    assert_eq!(back.model, "x");
}

// ---------------------------------------------------------------------------
// CategoryConfig
// ---------------------------------------------------------------------------

#[test]
fn category_config_field_access() {
    let c = CategoryConfig {
        model: "gpt-4o-mini".into(),
        endpoint: Some("https://api.openai.com".into()),
        api_key: Some("key".into()),
        strategy: Some(ModelRouteStrategy::CostOptimized),
    };
    assert_eq!(c.model, "gpt-4o-mini");
    assert_eq!(c.strategy, Some(ModelRouteStrategy::CostOptimized));
}

#[test]
fn category_config_no_strategy() {
    let c = CategoryConfig {
        model: "x".into(),
        endpoint: None,
        api_key: None,
        strategy: None,
    };
    assert!(c.strategy.is_none());
}

// ---------------------------------------------------------------------------
// BudgetMode
// ---------------------------------------------------------------------------

#[test]
fn budget_mode_default_is_warning() {
    let m: BudgetMode = Default::default();
    assert_eq!(m, BudgetMode::Warning);
}

#[test]
fn budget_mode_equality() {
    assert_eq!(BudgetMode::Warning, BudgetMode::Warning);
    assert_eq!(BudgetMode::Strict, BudgetMode::Strict);
    assert_ne!(BudgetMode::Warning, BudgetMode::Strict);
}

#[test]
fn budget_mode_serde() {
    for m in [BudgetMode::Warning, BudgetMode::Strict] {
        let j = serde_json::to_string(&m).unwrap();
        let back: BudgetMode = serde_json::from_str(&j).unwrap();
        assert_eq!(back, m);
    }
}

// ---------------------------------------------------------------------------
// EmbeddingConfig
// ---------------------------------------------------------------------------

#[test]
fn embedding_config_default_fields() {
    let e = EmbeddingConfig {
        model: "text-embedding-3-small".into(),
        endpoint: None,
        provider: "dummy".into(),
        allow_dummy_embeddings: false,
    };
    assert_eq!(e.model, "text-embedding-3-small");
    assert_eq!(e.provider, "dummy");
    assert!(!e.allow_dummy_embeddings);
}

#[test]
fn embedding_config_allow_dummy() {
    let e = EmbeddingConfig {
        model: "x".into(),
        endpoint: None,
        provider: "openai".into(),
        allow_dummy_embeddings: true,
    };
    assert!(e.allow_dummy_embeddings);
}

#[test]
fn embedding_config_serde() {
    let e = EmbeddingConfig::default();
    let j = serde_json::to_string(&e).unwrap();
    let back: EmbeddingConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.model, e.model);
}

// ---------------------------------------------------------------------------
// LlmConfig + default_llm_config
// ---------------------------------------------------------------------------

#[test]
fn llm_config_default() {
    let c = LlmConfig::default();
    assert_eq!(c.model, "gpt-4o");
    assert_eq!(c.endpoint, "http://localhost:8000/v1/chat/completions");
    assert_eq!(c.api_key, "");
    assert!(c.agents.is_empty());
    assert!(c.categories.is_empty());
    assert!(c.providers.is_empty());
}

#[test]
fn llm_config_default_timeouts() {
    let c = LlmConfig::default();
    assert_eq!(c.fast_fail_cooling_secs, 30);
    assert_eq!(c.request_timeout_secs, 5);
    assert_eq!(c.connect_timeout_secs, 2);
}

#[test]
fn llm_config_default_budget_mode() {
    let c = LlmConfig::default();
    assert_eq!(c.budget_mode, BudgetMode::Warning);
}

#[test]
fn llm_config_default_embedding() {
    let c = LlmConfig::default();
    assert_eq!(c.embedding.model, "text-embedding-3-small");
    assert_eq!(c.embedding.provider, "dummy");
}

#[test]
fn default_llm_config_function() {
    let c = default_llm_config();
    assert_eq!(c.model, "gpt-4o");
    assert_eq!(c.request_timeout_secs, 5);
}

#[test]
fn llm_config_serde_round_trip() {
    let c = LlmConfig {
        api_key: "sk-test".into(),
        endpoint: "http://localhost:8000".into(),
        model: "gpt-4o".into(),
        embedding: EmbeddingConfig::default(),
        budget_mode: BudgetMode::Strict,
        agents: Default::default(),
        categories: Default::default(),
        providers: vec![],
        fast_fail_cooling_secs: 60,
        request_timeout_secs: 10,
        connect_timeout_secs: 3,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: LlmConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.api_key, "sk-test");
    assert_eq!(back.budget_mode, BudgetMode::Strict);
    assert_eq!(back.request_timeout_secs, 10);
}

#[test]
fn llm_config_with_agents() {
    let mut c = LlmConfig::default();
    c.agents.insert(
        "metis".into(),
        AgentLlmOverride {
            model: "claude-3".into(),
            endpoint: None,
            api_key: None,
        },
    );
    assert_eq!(c.agents.len(), 1);
    assert_eq!(c.agents.get("metis").unwrap().model, "claude-3");
}
