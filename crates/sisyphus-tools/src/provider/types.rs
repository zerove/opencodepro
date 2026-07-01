//! Shared types for the provider system.

use serde::{Deserialize, Serialize};

/// Result of a chat completion call.
#[derive(Debug, Clone)]
pub struct ProviderResponse {
    /// The generated text content.
    pub content: String,
    /// Model used (may differ from requested model if provider aliases).
    pub model: String,
    /// Input token count (if known).
    pub input_tokens: Option<u64>,
    /// Output token count (if known).
    pub output_tokens: Option<u64>,
}

/// Error from a provider call.
#[derive(Debug, Clone)]
pub enum ProviderError {
    /// Transient error — the caller should retry or fall back.
    Transient(String),
    /// Permanent error — do not retry with this provider.
    Permanent(String),
    /// Rate limited — wait before retrying.
    RateLimited(String),
    /// Authentication failure.
    Auth(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Transient(msg) => write!(f, "Transient: {msg}"),
            ProviderError::Permanent(msg) => write!(f, "Permanent: {msg}"),
            ProviderError::RateLimited(msg) => write!(f, "Rate limited: {msg}"),
            ProviderError::Auth(msg) => write!(f, "Auth: {msg}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// A single chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

impl ChatRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChatRole::System => "system",
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
        }
    }
}

/// Configuration for LLM provider retries.
pub struct RetryConfig {
    pub max_retries: u32,
    pub base_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1000,
        }
    }
}
