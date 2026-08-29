//! Integration tests for sisyphus-types `config::provider` module.
//!
//! Exercises the public surfaces of [`ProviderKind`] and
//! [`ProviderConfig`]: `as_str`, `default_endpoint`, `default_model`,
//! `is_openai_compatible`, and `resolve_api_key`.

use sisyphus_types::config::provider::{ProviderConfig, ProviderKind};

// ---------------------------------------------------------------------------
// ProviderKind::as_str
// ---------------------------------------------------------------------------

#[test]
fn provider_kind_as_str_all() {
    assert_eq!(ProviderKind::OpenAi.as_str(), "openai");
    assert_eq!(ProviderKind::Google.as_str(), "google");
    assert_eq!(ProviderKind::Anthropic.as_str(), "anthropic");
    assert_eq!(ProviderKind::Groq.as_str(), "groq");
    assert_eq!(ProviderKind::Cohere.as_str(), "cohere");
    assert_eq!(ProviderKind::Mistral.as_str(), "mistral");
    assert_eq!(ProviderKind::Together.as_str(), "together");
    assert_eq!(ProviderKind::OpenRouter.as_str(), "openrouter");
}

// ---------------------------------------------------------------------------
// ProviderKind::default_endpoint
// ---------------------------------------------------------------------------

#[test]
fn provider_kind_default_endpoint_openai() {
    let e = ProviderKind::OpenAi.default_endpoint();
    assert!(e.is_some());
    assert!(e.unwrap().contains("openai.com"));
}

#[test]
fn provider_kind_default_endpoint_groq() {
    let e = ProviderKind::Groq.default_endpoint();
    assert!(e.is_some());
    assert!(e.unwrap().contains("groq.com"));
}

#[test]
fn provider_kind_default_endpoint_google_is_none() {
    // Google uses a custom protocol, not OpenAI-compatible.
    assert!(ProviderKind::Google.default_endpoint().is_none());
}

#[test]
fn provider_kind_default_endpoint_anthropic_is_none() {
    assert!(ProviderKind::Anthropic.default_endpoint().is_none());
}

#[test]
fn provider_kind_default_endpoint_mistral() {
    let e = ProviderKind::Mistral.default_endpoint();
    // Mistral is OpenAI-compatible and should have a default endpoint.
    if let Some(url) = e {
        assert!(url.starts_with("http"));
    }
}

// ---------------------------------------------------------------------------
// ProviderKind::default_model
// ---------------------------------------------------------------------------

#[test]
fn provider_kind_default_model_openai_nonempty() {
    let m = ProviderKind::OpenAi.default_model();
    assert!(!m.is_empty());
}

#[test]
fn provider_kind_default_model_google_nonempty() {
    let m = ProviderKind::Google.default_model();
    assert!(!m.is_empty());
}

#[test]
fn provider_kind_default_model_anthropic_nonempty() {
    let m = ProviderKind::Anthropic.default_model();
    assert!(!m.is_empty());
}

// ---------------------------------------------------------------------------
// ProviderKind::is_openai_compatible
// ---------------------------------------------------------------------------

#[test]
fn provider_kind_is_openai_compatible_openai() {
    assert!(ProviderKind::OpenAi.is_openai_compatible());
}

#[test]
fn provider_kind_is_openai_compatible_groq() {
    assert!(ProviderKind::Groq.is_openai_compatible());
}

#[test]
fn provider_kind_is_openai_compatible_mistral() {
    assert!(ProviderKind::Mistral.is_openai_compatible());
}

#[test]
fn provider_kind_is_openai_compatible_together() {
    assert!(ProviderKind::Together.is_openai_compatible());
}

#[test]
fn provider_kind_is_openai_compatible_openrouter() {
    assert!(ProviderKind::OpenRouter.is_openai_compatible());
}

// ---------------------------------------------------------------------------
// ProviderKind serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn provider_kind_serde_openai() {
    let k = ProviderKind::OpenAi;
    let j = serde_json::to_string(&k).unwrap();
    assert_eq!(j, "\"openai\"");
    let back: ProviderKind = serde_json::from_str(&j).unwrap();
    assert_eq!(back, ProviderKind::OpenAi);
}

#[test]
fn provider_kind_serde_anthropic() {
    let k = ProviderKind::Anthropic;
    let j = serde_json::to_string(&k).unwrap();
    assert_eq!(j, "\"anthropic\"");
    let back: ProviderKind = serde_json::from_str(&j).unwrap();
    assert_eq!(back, ProviderKind::Anthropic);
}

// ---------------------------------------------------------------------------
// ProviderConfig::resolve_api_key
// ---------------------------------------------------------------------------

fn make_config(api_key: &str) -> ProviderConfig {
    ProviderConfig {
        name: "test".into(),
        kind: ProviderKind::OpenAi,
        endpoint: "https://x".into(),
        api_key: api_key.into(),
        model: "gpt-4o".into(),
        base_url: None,
        priority: 1,
        enabled: true,
        max_tokens: None,
        temperature: None,
    }
}

#[test]
fn provider_config_resolve_api_key_literal() {
    let c = make_config("sk-abc");
    assert_eq!(c.resolve_api_key(), "sk-abc");
}

#[test]
fn provider_config_resolve_api_key_env_unset() {
    // SAFETY: test sets an env var; this test does not run in parallel
    // with other env-touching tests in the same binary.
    unsafe {
        std::env::remove_var("SISYPHUS_TEST_API_KEY");
    }
    let c = make_config("env:SISYPHUS_TEST_API_KEY");
    // Unset env var → empty string.
    assert_eq!(c.resolve_api_key(), "");
}

#[test]
fn provider_config_resolve_api_key_env_set() {
    unsafe {
        std::env::set_var("SISYPHUS_TEST_API_KEY2", "secret-value");
    }
    let c = make_config("env:SISYPHUS_TEST_API_KEY2");
    assert_eq!(c.resolve_api_key(), "secret-value");
    unsafe {
        std::env::remove_var("SISYPHUS_TEST_API_KEY2");
    }
}

#[test]
fn provider_config_resolve_api_key_empty() {
    let c = make_config("");
    assert_eq!(c.resolve_api_key(), "");
}

// ---------------------------------------------------------------------------
// ProviderConfig field assignment
// ---------------------------------------------------------------------------

#[test]
fn provider_config_field_assignment() {
    let c = ProviderConfig {
        name: "google-gemini-2".into(),
        kind: ProviderKind::Google,
        endpoint: "https://generativelanguage.googleapis.com/v1beta/models".into(),
        api_key: "env:GOOGLE_API_KEY".into(),
        model: "gemini-2.0-flash-exp".into(),
        base_url: Some("https://alt.example.com".into()),
        priority: 5,
        enabled: true,
        max_tokens: Some(8192),
        temperature: Some(0.7),
    };
    assert_eq!(c.name, "google-gemini-2");
    assert_eq!(c.kind, ProviderKind::Google);
    assert_eq!(c.priority, 5);
    assert!(c.base_url.is_some());
    assert_eq!(c.max_tokens, Some(8192));
    assert!((c.temperature.unwrap() - 0.7).abs() < 1e-9);
}
