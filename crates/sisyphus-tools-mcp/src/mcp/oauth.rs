//! OAuth2 authentication for MCP transports.
//!
//! Implements the **client_credentials** grant (RFC 6749 §4.4) for
//! authenticating the local MCP client against an OAuth2 authorization
//! server before issuing requests.
//!
//! ## Usage
//!
//! ```no_run
//! use sisyphus_tools_mcp::mcp::oauth::OAuth2ClientCredentials;
//!
//! let oauth = OAuth2ClientCredentials::new(
//!     "https://auth.example.com/oauth/token",  // token endpoint
//!     "my-client-id",
//!     "my-client-secret",
//!     Some("read:tools"),                            // scope
//! );
//! let token = oauth.fetch_token().await?;
//! ```
//!
//! Tokens are cached in-memory and reused until expiry (per the `expires_in`
//! hint or a 5-minute safety margin).

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// OAuth2 access token response (RFC 6749 §5.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// Cached token + expiry tracker.
#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at: Option<Instant>,
}

/// Errors during OAuth2 token fetch.
#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("Token endpoint returned non-success status {0}: {1}")]
    BadStatus(u16, String),
    #[error("Failed to parse token response: {0}")]
    Parse(String),
    #[error("Missing access_token in response")]
    MissingToken,
}

impl std::fmt::Display for CachedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<redacted:{}>",
            self.access_token.chars().take(8).collect::<String>()
        )
    }
}

/// Client for the OAuth2 client_credentials grant.
///
/// The token is fetched on the first call to `access_token()` and
/// transparently refreshed on subsequent calls when expired.
#[derive(Debug)]
pub struct OAuth2ClientCredentials {
    token_endpoint: String,
    client_id: String,
    client_secret: String,
    scope: Option<String>,
    http: reqwest::Client,
    cache: Arc<Mutex<Option<CachedToken>>>,
}

impl OAuth2ClientCredentials {
    /// Create a new client_credentials provider.
    pub fn new(
        token_endpoint: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        scope: Option<&str>,
    ) -> Self {
        Self {
            token_endpoint: token_endpoint.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            scope: scope.map(String::from),
            http: reqwest::Client::new(),
            cache: Arc::new(Mutex::new(None)),
        }
    }

    /// Get a current access token, fetching a new one if necessary.
    pub async fn access_token(&self) -> Result<String, OAuthError> {
        // Check cache first (fast path).
        {
            let cache = self.cache.lock();
            if let Some(cached) = cache.as_ref() {
                let still_valid = cached
                    .expires_at
                    .map(|t| t > Instant::now())
                    .unwrap_or(true);
                if still_valid {
                    return Ok(cached.access_token.clone());
                }
            }
        }

        // Fetch a new token.
        let response = self.fetch_token().await?;
        let expires_at = response.expires_in.map(|secs| {
            // 5-minute safety margin per RFC 6749 §5.1 guidance.
            let safe_secs = secs.saturating_sub(300).max(1);
            Instant::now() + Duration::from_secs(safe_secs)
        });
        let token = response.access_token;
        if token.is_empty() {
            return Err(OAuthError::MissingToken);
        }

        *self.cache.lock() = Some(CachedToken {
            access_token: token.clone(),
            expires_at,
        });
        Ok(token)
    }

    /// Force a new token fetch, ignoring the cache.
    pub async fn fetch_token(&self) -> Result<TokenResponse, OAuthError> {
        let mut form = vec![
            ("grant_type", "client_credentials".to_string()),
            ("client_id", self.client_id.clone()),
            ("client_secret", self.client_secret.clone()),
        ];
        if let Some(scope) = &self.scope {
            form.push(("scope", scope.clone()));
        }

        let resp = self
            .http
            .post(&self.token_endpoint)
            .header("Accept", "application/json")
            .form(&form)
            .send()
            .await
            .map_err(|e| OAuthError::Http(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OAuthError::BadStatus(status.as_u16(), body));
        }

        let token: TokenResponse = resp
            .json()
            .await
            .map_err(|e| OAuthError::Parse(e.to_string()))?;
        Ok(token)
    }

    /// Invalidate the cached token (e.g., after a 401 from the resource server).
    pub fn invalidate(&self) {
        *self.cache.lock() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth2_client_creation() {
        let oauth = OAuth2ClientCredentials::new(
            "https://auth.example.com/oauth/token",
            "client-id",
            "client-secret",
            Some("read:tools"),
        );
        assert!(oauth.cache.lock().is_none());
    }

    #[test]
    fn test_oauth2_no_scope() {
        let oauth =
            OAuth2ClientCredentials::new("https://auth.example.com/token", "id", "secret", None);
        assert!(oauth.scope.is_none());
    }

    #[test]
    fn test_token_response_parse_minimal() {
        // 最小响应只含 access_token
        let json = r#"{"access_token": "abc123"}"#;
        let token: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(token.access_token, "abc123");
        assert_eq!(token.token_type, "");
        assert!(token.expires_in.is_none());
    }

    #[test]
    fn test_token_response_parse_full() {
        let json = r#"{
            "access_token": "xyz",
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": "read write"
        }"#;
        let token: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(token.access_token, "xyz");
        assert_eq!(token.token_type, "Bearer");
        assert_eq!(token.expires_in, Some(3600));
        assert_eq!(token.scope.as_deref(), Some("read write"));
    }

    #[tokio::test]
    async fn test_invalidate_clears_cache() {
        let oauth =
            OAuth2ClientCredentials::new("https://auth.example.com/token", "id", "secret", None);
        *oauth.cache.lock() = Some(CachedToken {
            access_token: "stale".into(),
            expires_at: Some(Instant::now() + Duration::from_secs(60)),
        });
        assert!(oauth.cache.lock().is_some());
        oauth.invalidate();
        assert!(oauth.cache.lock().is_none());
    }

    #[tokio::test]
    async fn test_access_token_uses_cache_when_valid() {
        let oauth =
            OAuth2ClientCredentials::new("https://auth.example.com/token", "id", "secret", None);
        *oauth.cache.lock() = Some(CachedToken {
            access_token: "cached-token".into(),
            expires_at: Some(Instant::now() + Duration::from_secs(60)),
        });
        let token = oauth.access_token().await.unwrap();
        assert_eq!(token, "cached-token");
    }

    #[tokio::test]
    async fn test_access_token_refreshes_when_expired() {
        let oauth = OAuth2ClientCredentials::new(
            "https://127.0.0.1:1/nonexistent", // 不可达, 应触发 HTTP 错误
            "id",
            "secret",
            None,
        );
        *oauth.cache.lock() = Some(CachedToken {
            access_token: "old".into(),
            expires_at: Some(Instant::now() - Duration::from_secs(1)),
        });
        // 过期应触发网络请求, 因为端点不可达会失败
        let result = oauth.access_token().await;
        assert!(result.is_err(), "过期应触发网络请求并失败");
    }
}
