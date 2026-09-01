//! Integration tests for sisyphus-tools-llm `provider::types`.
//!
//! Exercises the public types: `ChatMessage`, `ChatRole`,
//! `ProviderResponse`, `ProviderError`, and `RetryConfig`. All
//! are pure data types with serde derives, so the tests focus on
//! serialization round-trips, equality, and the `ChatRole::as_str`
//! helper.

use sisyphus_tools_llm::provider::{
    ChatMessage, ChatRole, ProviderError, ProviderResponse, RetryConfig,
};

// ---------------------------------------------------------------------------
// ChatRole
// ---------------------------------------------------------------------------

#[test]
fn chat_role_as_str_returns_lowercase() {
    assert_eq!(ChatRole::System.as_str(), "system");
    assert_eq!(ChatRole::User.as_str(), "user");
    assert_eq!(ChatRole::Assistant.as_str(), "assistant");
}

#[test]
fn chat_role_equality() {
    assert_eq!(ChatRole::System, ChatRole::System);
    assert_ne!(ChatRole::System, ChatRole::User);
    assert_ne!(ChatRole::User, ChatRole::Assistant);
}

#[test]
fn chat_role_serialization_roundtrip() {
    for role in [ChatRole::System, ChatRole::User, ChatRole::Assistant] {
        let json = serde_json::to_string(&role).expect("ok");
        let parsed: ChatRole = serde_json::from_str(&json).expect("ok");
        assert_eq!(role, parsed);
    }
}

// ---------------------------------------------------------------------------
// ChatMessage
// ---------------------------------------------------------------------------

#[test]
fn chat_message_serialization_roundtrip() {
    let m = ChatMessage {
        role: ChatRole::User,
        content: "Hello, world!".to_string(),
    };
    let json = serde_json::to_string(&m).expect("");
    let parsed: ChatMessage = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.role, ChatRole::User);
    assert_eq!(parsed.content, "Hello, world!");
}

#[test]
fn chat_message_clone_preserves_content() {
    let m = ChatMessage {
        role: ChatRole::System,
        content: "Be helpful".to_string(),
    };
    let cloned = m.clone();
    assert_eq!(m.content, cloned.content);
    assert_eq!(m.role, cloned.role);
}

#[test]
fn chat_message_with_unicode_content() {
    let m = ChatMessage {
        role: ChatRole::User,
        content: "你好 🌍".to_string(),
    };
    let json = serde_json::to_string(&m).expect("");
    let parsed: ChatMessage = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.content, "你好 🌍");
}

#[test]
fn chat_message_with_empty_content() {
    let m = ChatMessage {
        role: ChatRole::User,
        content: String::new(),
    };
    let json = serde_json::to_string(&m).expect("");
    let parsed: ChatMessage = serde_json::from_str(&json).expect("");
    assert!(parsed.content.is_empty());
}

// ---------------------------------------------------------------------------
// ProviderResponse
// ---------------------------------------------------------------------------

#[test]
fn provider_response_with_all_fields() {
    let r = ProviderResponse {
        content: "response text".into(),
        model: "gpt-4".into(),
        input_tokens: Some(100),
        output_tokens: Some(50),
    };
    assert_eq!(r.content, "response text");
    assert_eq!(r.model, "gpt-4");
    assert_eq!(r.input_tokens, Some(100));
    assert_eq!(r.output_tokens, Some(50));
}

#[test]
fn provider_response_with_no_token_counts() {
    let r = ProviderResponse {
        content: "x".into(),
        model: "y".into(),
        input_tokens: None,
        output_tokens: None,
    };
    assert!(r.input_tokens.is_none());
    assert!(r.output_tokens.is_none());
}

#[test]
fn provider_response_clone() {
    let r = ProviderResponse {
        content: "x".into(),
        model: "y".into(),
        input_tokens: Some(1),
        output_tokens: Some(2),
    };
    let cloned = r.clone();
    assert_eq!(r.content, cloned.content);
    assert_eq!(r.input_tokens, cloned.input_tokens);
}

// ---------------------------------------------------------------------------
// ProviderError
// ---------------------------------------------------------------------------

#[test]
fn provider_error_display_messages() {
    let transient = ProviderError::Transient("network blip".into());
    let permanent = ProviderError::Permanent("bad request".into());
    let rate = ProviderError::RateLimited("too fast".into());
    let auth = ProviderError::Auth("bad key".into());
    assert!(transient.to_string().contains("Transient"));
    assert!(transient.to_string().contains("network blip"));
    assert!(permanent.to_string().contains("Permanent"));
    assert!(permanent.to_string().contains("bad request"));
    assert!(rate.to_string().contains("Rate limited"));
    assert!(auth.to_string().contains("Auth"));
}

#[test]
fn provider_error_preserves_inner_message() {
    let e = ProviderError::Auth("invalid api key".into());
    assert_eq!(e.to_string(), "Auth: invalid api key");
}

#[test]
fn provider_error_clone_preserves_payload() {
    let e = ProviderError::Transient("timeout".into());
    let cloned = e.clone();
    assert_eq!(e.to_string(), cloned.to_string());
}

// ---------------------------------------------------------------------------
// RetryConfig
// ---------------------------------------------------------------------------

#[test]
fn retry_config_default_values() {
    let cfg = RetryConfig::default();
    assert_eq!(cfg.max_retries, 3);
    assert_eq!(cfg.base_delay_ms, 1000);
}

#[test]
fn retry_config_with_custom_values() {
    let cfg = RetryConfig {
        max_retries: 5,
        base_delay_ms: 500,
    };
    assert_eq!(cfg.max_retries, 5);
    assert_eq!(cfg.base_delay_ms, 500);
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn chat_role_as_str() {
    use sisyphus_tools_llm::provider::types::ChatRole;
    assert_eq!(ChatRole::System.as_str(), "system");
    assert_eq!(ChatRole::User.as_str(), "user");
    assert_eq!(ChatRole::Assistant.as_str(), "assistant");
}

#[test]
fn chat_role_debug() {
    use sisyphus_tools_llm::provider::types::ChatRole;
    let _ = format!("{:?}", ChatRole::System);
    let _ = format!("{:?}", ChatRole::User);
    let _ = format!("{:?}", ChatRole::Assistant);
}

#[test]
fn chat_message_with_long_content() {
    use sisyphus_tools_llm::provider::types::{ChatMessage, ChatRole};
    let long = "x".repeat(10_000);
    let m = ChatMessage {
        role: ChatRole::User,
        content: long.clone(),
    };
    assert_eq!(m.content.len(), 10_000);
    let j = serde_json::to_string(&m).unwrap();
    let back: ChatMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.content.len(), 10_000);
}

#[test]
fn chat_message_role_variants() {
    use sisyphus_tools_llm::provider::types::{ChatMessage, ChatRole};
    for role in [ChatRole::System, ChatRole::User, ChatRole::Assistant] {
        let m = ChatMessage {
            role: role.clone(),
            content: "test".into(),
        };
        let j = serde_json::to_string(&m).unwrap();
        let back: ChatMessage = serde_json::from_str(&j).unwrap();
        assert_eq!(back.role, role);
    }
}

#[test]
fn provider_response_with_unicode() {
    use sisyphus_tools_llm::provider::types::ProviderResponse;
    let r = ProviderResponse {
        content: "中文响应 🎉".into(),
        model: "gpt-α".into(),
        input_tokens: Some(10),
        output_tokens: Some(20),
    };
    assert_eq!(r.content, "中文响应 🎉");
    assert_eq!(r.model, "gpt-α");
}

#[test]
fn provider_response_with_zero_tokens() {
    use sisyphus_tools_llm::provider::types::ProviderResponse;
    let r = ProviderResponse {
        content: "".into(),
        model: "".into(),
        input_tokens: Some(0),
        output_tokens: Some(0),
    };
    assert_eq!(r.input_tokens, Some(0));
}

#[test]
fn provider_response_with_max_tokens() {
    use sisyphus_tools_llm::provider::types::ProviderResponse;
    let r = ProviderResponse {
        content: "x".into(),
        model: "m".into(),
        input_tokens: Some(u64::MAX),
        output_tokens: Some(u64::MAX),
    };
    assert_eq!(r.input_tokens, Some(u64::MAX));
}

#[test]
fn provider_error_clone() {
    use sisyphus_tools_llm::provider::types::ProviderError;
    let e = ProviderError::Transient("retry".into());
    let cloned = e.clone();
    let s1 = format!("{}", e);
    let s2 = format!("{}", cloned);
    assert_eq!(s1, s2);
}

#[test]
fn provider_error_debug() {
    use sisyphus_tools_llm::provider::types::ProviderError;
    let _ = format!("{:?}", ProviderError::Permanent("err".into()));
    let _ = format!("{:?}", ProviderError::RateLimited("rate".into()));
    let _ = format!("{:?}", ProviderError::Auth("auth".into()));
}

#[test]
fn provider_error_variants() {
    // rustc 1.98 clippy::useless_vec 修复：直接用数组
    let variants = [
        ProviderError::Transient("a".into()),
        ProviderError::Permanent("b".into()),
        ProviderError::RateLimited("c".into()),
        ProviderError::Auth("d".into()),
    ];
    // Each variant has distinct display
    let displays: Vec<String> = variants.iter().map(|v| format!("{}", v)).collect();
    let unique: std::collections::HashSet<_> = displays.iter().collect();
    assert_eq!(unique.len(), 4);
}

#[test]
fn retry_config_clone_or_default() {
    use sisyphus_tools_llm::provider::types::RetryConfig;
    let c = RetryConfig::default();
    // Just verify default doesn't panic
    let _ = c;
}
