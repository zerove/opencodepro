//! Integration tests for sisyphus-tools-llm `provider::registry`.
//!
//! Exercises the public [`ProviderRegistry`] API: register, get,
//! list, remove, fallback order, and `chat_with_provider` for
//! provider-not-found errors. Uses a mock `LlmProvider`
//! implementation to drive the registry without making real API
//! calls.

use async_trait::async_trait;
use sisyphus_tools_llm::provider::{ChatMessage, LlmProvider, ProviderError, ProviderResponse};
use sisyphus_tools_llm::provider::ProviderRegistry;
use sisyphus_types::ProviderKind;
use std::sync::Arc;

struct MockProvider {
    name: String,
    kind: ProviderKind,
    model: String,
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn kind(&self) -> ProviderKind {
        self.kind
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn model(&self) -> &str {
        &self.model
    }
    async fn chat(
        &self,
        _system: &str,
        _messages: &[ChatMessage],
        _json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            content: format!("mock:{}", self.name),
            model: self.model.clone(),
            input_tokens: Some(1),
            output_tokens: Some(1),
        })
    }
}

fn make_provider(name: &str) -> Arc<dyn LlmProvider> {
    Arc::new(MockProvider {
        name: name.into(),
        kind: ProviderKind::OpenAi,
        model: format!("model-{name}"),
    })
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn new_registry_starts_empty() {
    let r = ProviderRegistry::new();
    assert!(r.list().is_empty());
    assert!(r.fallback_order().is_empty());
}

#[test]
fn default_registry_matches_new() {
    let r = ProviderRegistry::default();
    assert!(r.list().is_empty());
}

// ---------------------------------------------------------------------------
// register / get / list
// ---------------------------------------------------------------------------

#[test]
fn register_adds_provider() {
    let mut r = ProviderRegistry::new();
    r.register(make_provider("p1"));
    assert_eq!(r.list(), vec!["p1".to_string()]);
    assert!(r.get("p1").is_some());
}

#[test]
fn get_unknown_returns_none() {
    let r = ProviderRegistry::new();
    assert!(r.get("nonexistent").is_none());
}

#[test]
fn register_multiple_providers() {
    let mut r = ProviderRegistry::new();
    r.register(make_provider("a"));
    r.register(make_provider("b"));
    r.register(make_provider("c"));
    assert_eq!(r.list().len(), 3);
    assert!(r.get("a").is_some());
    assert!(r.get("b").is_some());
    assert!(r.get("c").is_some());
}

#[test]
fn register_replaces_existing() {
    let mut r = ProviderRegistry::new();
    r.register(make_provider("p1"));
    r.register(make_provider("p1"));
    // list() uses HashMap.keys() — duplicated name doesn't duplicate.
    assert_eq!(r.list().len(), 1);
    // Fallback order shouldn't have duplicate either.
    let order = r.fallback_order();
    assert_eq!(order.iter().filter(|n| *n == "p1").count(), 1);
}

#[test]
fn register_appends_to_fallback_order_if_new() {
    let mut r = ProviderRegistry::new();
    r.register(make_provider("a"));
    r.register(make_provider("b"));
    let order = r.fallback_order();
    assert_eq!(order, &["a".to_string(), "b".to_string()]);
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

#[test]
fn remove_existing_returns_true() {
    let mut r = ProviderRegistry::new();
    r.register(make_provider("p1"));
    assert!(r.remove("p1"));
    assert!(r.get("p1").is_none());
    assert!(r.fallback_order().is_empty());
}

#[test]
fn remove_nonexistent_returns_false() {
    let mut r = ProviderRegistry::new();
    assert!(!r.remove("never"));
}

#[test]
fn remove_only_removes_target_from_fallback() {
    let mut r = ProviderRegistry::new();
    r.register(make_provider("a"));
    r.register(make_provider("b"));
    r.register(make_provider("c"));
    r.remove("b");
    let order = r.fallback_order();
    assert_eq!(order, &["a".to_string(), "c".to_string()]);
}

// ---------------------------------------------------------------------------
// set_fallback_order
// ---------------------------------------------------------------------------

#[test]
fn set_fallback_order_replaces_existing() {
    let mut r = ProviderRegistry::new();
    r.register(make_provider("a"));
    r.register(make_provider("b"));
    r.set_fallback_order(vec!["b".into(), "a".into()]);
    assert_eq!(r.fallback_order(), &["b".to_string(), "a".to_string()]);
}

#[test]
fn set_fallback_order_with_empty_clears() {
    let mut r = ProviderRegistry::new();
    r.register(make_provider("a"));
    r.set_fallback_order(vec![]);
    assert!(r.fallback_order().is_empty());
}

#[test]
fn set_fallback_order_with_unknown_names_allowed() {
    // The registry doesn't validate that names in fallback_order
    // correspond to registered providers; this is the caller's job.
    let mut r = ProviderRegistry::new();
    r.set_fallback_order(vec!["ghost".into()]);
    assert_eq!(r.fallback_order(), &["ghost".to_string()]);
}

// ---------------------------------------------------------------------------
// chat_with_provider (error path)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_with_provider_unknown_returns_error() {
    let r = ProviderRegistry::new();
    let result = r
        .chat_with_provider("nonexistent", "system", &[], false)
        .await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// chat_with_fallback (error path)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_with_fallback_empty_returns_error() {
    let r = ProviderRegistry::new();
    let result = r.chat_with_fallback("system", &[], false).await;
    assert!(result.is_err());
}
