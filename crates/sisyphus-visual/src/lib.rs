//! Sisyphus Visual Perception system for multi-modal cognitive synergy.
//!
//! Provides [`VisualPerceptionProvider`] trait for image analysis and
//! embedding, with concrete implementations for HTTP-based VLM APIs
//! and a mock provider for testing.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use sisyphus_types::graph::NodeKind;
use sisyphus_tools_llm::LlmClient;
use thiserror::Error;

// ── Errors ───────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum VisualError {
    #[error("VLM provider error: {0}")]
    ProviderError(String),
    #[error("Image processing error: {0}")]
    ProcessingError(String),
    #[error("Invalid visual request: {0}")]
    InvalidRequest(String),
    #[error("HTTP error: {0}")]
    HttpError(String),
    #[error("Timeout: {0}")]
    Timeout(String),
}

// ── Types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualAnalysisResult {
    pub description: String,
    pub detected_elements: Vec<DetectedElement>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedElement {
    pub label: String,
    pub bounding_box: [f32; 4], // [x, y, w, h]
    pub confidence: f32,
}

/// Configuration for HTTP-based VLM providers.
#[derive(Debug, Clone)]
pub struct VlmProviderConfig {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub max_retries: u32,
    pub timeout_secs: u64,
}

impl Default for VlmProviderConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            endpoint: "https://api.openai.com/v1/chat/completions".into(),
            model: "gpt-4o".into(),
            max_retries: 3,
            timeout_secs: 30,
        }
    }
}

// ── Provider trait ───────────────────────────────────────────────────────

#[async_trait]
pub trait VisualPerceptionProvider: Send + Sync {
    async fn analyze_image(
        &self, 
        image_data: &[u8], 
        prompt: &str
    ) -> Result<VisualAnalysisResult, VisualError>;

    async fn get_embedding(
        &self,
        image_data: &[u8],
    ) -> Result<Vec<f32>, VisualError>;
}

// ── HTTP VLM Provider ───────────────────────────────────────────────────

/// Real VLM provider that calls OpenAI-compatible Vision APIs via HTTP.
pub struct HttpVlmProvider {
    config: VlmProviderConfig,
    client: reqwest::Client,
}

impl HttpVlmProvider {
    pub fn new(config: VlmProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("failed to build HTTP client");
        Self { config, client }
    }
}

#[async_trait]
impl VisualPerceptionProvider for HttpVlmProvider {
    async fn analyze_image(
        &self, 
        image_data: &[u8], 
        prompt: &str
    ) -> Result<VisualAnalysisResult, VisualError> {
        let base64_image = base64_encode(image_data);

        let request_body = serde_json::json!({
            "model": self.config.model,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": prompt
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/png;base64,{}", base64_image),
                            "detail": "high"
                        }
                    }
                ]
            }],
            "max_tokens": 1000
        });

        let response = self.client
            .post(&self.config.endpoint)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| VisualError::HttpError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(VisualError::ProviderError(
                format!("HTTP {}: {}", status, body)
            ));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| VisualError::HttpError(e.to_string()))?;

        // Extract content from OpenAI-style response
        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("No content in response");

        Ok(VisualAnalysisResult {
            description: content.to_string(),
            detected_elements: vec![], // TODO: parse structured output
            confidence: 0.9,
        })
    }

    async fn get_embedding(
        &self,
        _image_data: &[u8],
    ) -> Result<Vec<f32>, VisualError> {
        // Embedding endpoint would be different; for now return error
        Err(VisualError::ProviderError(
            "Image embedding not yet implemented for HTTP provider".into()
        ))
    }
}

// ── Mock provider (for testing) ─────────────────────────────────────────

pub struct SisyphusVisualProvider {
    _llm_client: Arc<dyn LlmClient>,
}

impl SisyphusVisualProvider {
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self { _llm_client: llm_client }
    }
}

#[async_trait]
impl VisualPerceptionProvider for SisyphusVisualProvider {
    async fn analyze_image(
        &self, 
        image_data: &[u8], 
        prompt: &str
    ) -> Result<VisualAnalysisResult, VisualError> {
        tracing::info!("VLM analyzing image ({} bytes) with prompt: {}", image_data.len(), prompt);
        Ok(VisualAnalysisResult {
            description: format!("Simulated analysis for prompt: {}", prompt),
            detected_elements: vec![],
            confidence: 0.9,
        })
    }

    async fn get_embedding(
        &self,
        _image_data: &[u8],
    ) -> Result<Vec<f32>, VisualError> {
        Ok(vec![0.1; 512])
    }
}

// ── VisualPerceptionManager ──────────────────────────────────────────────

pub struct VisualPerceptionManager {
    provider: Arc<dyn VisualPerceptionProvider>,
    _llm_client: Arc<dyn LlmClient>,
}

impl VisualPerceptionManager {
    pub fn new(provider: Arc<dyn VisualPerceptionProvider>, llm_client: Arc<dyn LlmClient>) -> Self {
        Self { provider, _llm_client: llm_client }
    }

    /// Processes a visual request based on the NodeKind.
    pub async fn process_visual_node(
        &self,
        kind: &NodeKind,
        image_data: &[u8],
    ) -> Result<String, VisualError> {
        match kind {
            NodeKind::VisualAnalysis { prompt, .. } => {
                let result = self.provider.analyze_image(image_data, prompt).await?;
                Ok(serde_json::to_string(&result).map_err(|e| VisualError::ProcessingError(e.to_string()))?)
            }
            NodeKind::VisualAction { action_type, target_selector, params } => {
                let prompt = format!(
                    "Locate the element matching selector '{}' for action '{}' with params {}. Return coordinates.",
                    target_selector, action_type, params
                );
                let result = self.provider.analyze_image(image_data, &prompt).await?;
                Ok(serde_json::to_string(&result).map_err(|e| VisualError::ProcessingError(e.to_string()))?)
            }
            _ => Err(VisualError::InvalidRequest("Node is not a visual node".into())),
        }
    }

    pub async fn get_embedding(&self, image_data: &[u8]) -> Result<Vec<f32>, VisualError> {
        self.provider.get_embedding(image_data).await
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Simple base64 encoding (no dependency needed).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_tools_llm::DummyLlmClient;

    #[test]
    fn test_visual_error_display() {
        let err = VisualError::ProviderError("test".into());
        assert!(err.to_string().contains("test"));

        let err = VisualError::HttpError("connection failed".into());
        assert!(err.to_string().contains("connection failed"));
    }

    #[test]
    fn test_visual_analysis_result_serialization() {
        let result = VisualAnalysisResult {
            description: "test".into(),
            detected_elements: vec![DetectedElement {
                label: "button".into(),
                bounding_box: [10.0, 20.0, 100.0, 50.0],
                confidence: 0.95,
            }],
            confidence: 0.9,
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: VisualAnalysisResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.description, "test");
        assert_eq!(deserialized.detected_elements.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_provider_analyze() {
        let llm = Arc::new(DummyLlmClient);
        let provider = Arc::new(SisyphusVisualProvider::new(llm));
        let result = provider.analyze_image(b"test image", "describe this").await.unwrap();
        assert!(result.description.contains("Simulated"));
        assert_eq!(result.confidence, 0.9);
    }

    #[tokio::test]
    async fn test_mock_provider_embedding() {
        let llm = Arc::new(DummyLlmClient);
        let provider = Arc::new(SisyphusVisualProvider::new(llm));
        let embedding = provider.get_embedding(b"test").await.unwrap();
        assert_eq!(embedding.len(), 512);
    }

    #[test]
    fn test_vlm_config_default() {
        let config = VlmProviderConfig::default();
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.timeout_secs, 30);
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn test_http_provider_creation() {
        let config = VlmProviderConfig {
            api_key: "test-key".into(),
            endpoint: "https://api.example.com/v1/vision".into(),
            model: "test-model".into(),
            ..Default::default()
        };
        let provider = HttpVlmProvider::new(config);
        // Just verify it doesn't panic
        assert!(format!("{:?}", provider).contains("HttpVlmProvider"));
    }
}
