//! Sisyphus API client.

use crate::error::SdkError;
use crate::models::*;
use serde_json::Value;

/// Client for the Sisyphus REST API
pub struct SisyphusClient {
    base_url: String,
    http: reqwest::Client,
}

impl SisyphusClient {
    /// Create a new client pointing to the Sisyphus API server
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Create a client with custom HTTP client
    pub fn with_http_client(base_url: &str, http: reqwest::Client) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Health check
    pub async fn health(&self) -> Result<HealthResponse, SdkError> {
        let resp = self.http.get(self.url("/health")).send().await?;
        Ok(resp.json().await?)
    }

    /// Get state by key
    pub async fn get_state(&self, key: &str) -> Result<StateResponse, SdkError> {
        let resp = self
            .http
            .get(self.url(&format!("/api/v1/state/{}", key)))
            .send()
            .await?;

        match resp.status().as_u16() {
            200 => Ok(resp.json().await?),
            404 => Err(SdkError::NotFound(key.to_string())),
            status => Err(SdkError::ServerError {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }

    /// Set state
    pub async fn set_state(&self, key: &str, value: Value) -> Result<StateResponse, SdkError> {
        let resp = self
            .http
            .post(self.url(&format!("/api/v1/state/{}", key)))
            .json(&SetStateRequest { value })
            .send()
            .await?;
        Ok(resp.json().await?)
    }

    /// Delete state
    pub async fn delete_state(&self, key: &str) -> Result<(), SdkError> {
        let resp = self
            .http
            .delete(self.url(&format!("/api/v1/state/{}", key)))
            .send()
            .await?;
        match resp.status().as_u16() {
            204 => Ok(()),
            404 => Err(SdkError::NotFound(key.to_string())),
            status => Err(SdkError::ServerError {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }

    /// List state entries
    pub async fn list_state(
        &self,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<ListStateResponse, SdkError> {
        let mut url = self.url("/api/v1/state");
        let mut params = Vec::new();
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if let Some(o) = offset {
            params.push(format!("offset={}", o));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        let resp = self.http.get(&url).send().await?;
        Ok(resp.json().await?)
    }

    /// Search vectors
    pub async fn search_vectors(
        &self,
        query: &[f32],
        top_k: usize,
    ) -> Result<VectorSearchResponse, SdkError> {
        let resp = self
            .http
            .post(self.url("/api/v1/vectors/search"))
            .json(&VectorSearchRequest {
                query: query.to_vec(),
                top_k: Some(top_k),
                filter: None,
            })
            .send()
            .await?;
        Ok(resp.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = SisyphusClient::new("http://localhost:3000");
        assert_eq!(client.base_url, "http://localhost:3000");
    }

    #[test]
    fn test_url_construction() {
        let client = SisyphusClient::new("http://localhost:3000");
        assert_eq!(
            client.url("/api/v1/state"),
            "http://localhost:3000/api/v1/state"
        );
        // Trailing slash should be normalized
        let client2 = SisyphusClient::new("http://localhost:3000/");
        assert_eq!(client2.url("/health"), "http://localhost:3000/health");
    }
}
