//! Sisyphus Visual Perception system for multi-modal cognitive synergy.
//!
//! Provides [`VisualPerceptionProvider`] trait for image analysis and
//! embedding, with concrete implementations for HTTP-based VLM APIs
//! and a mock provider for testing.

pub use crate::helpers::{base64_encode, is_png, simulated_visual_state, PNG_MAGIC};

use metrics::counter;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sisyphus_tools_llm::LlmClient;
use sisyphus_types::graph::NodeKind;
use std::sync::Arc;
use thiserror::Error;

// ── Errors ───────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
/// Visual Error.
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
/// Visual Analysis Result.
pub struct VisualAnalysisResult {
    pub description: String,
    pub detected_elements: Vec<DetectedElement>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Detected Element.
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
/// Visual Perception Provider.
pub trait VisualPerceptionProvider: Send + Sync {
    async fn analyze_image(
        &self,
        image_data: &[u8],
        prompt: &str,
    ) -> Result<VisualAnalysisResult, VisualError>;

    async fn get_embedding(&self, image_data: &[u8]) -> Result<Vec<f32>, VisualError>;
}

// ── HTTP VLM Provider ───────────────────────────────────────────────────

/// Real VLM provider that calls OpenAI-compatible Vision APIs via HTTP.
pub struct HttpVlmProvider {
    config: VlmProviderConfig,
    client: reqwest::Client,
}

impl std::fmt::Debug for HttpVlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpVlmProvider")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl HttpVlmProvider {
    /// New.
    pub fn new(config: VlmProviderConfig) -> Result<Self, VisualError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| {
                VisualError::ProcessingError(format!("failed to build HTTP client: {e}"))
            })?;
        Ok(Self { config, client })
    }
}

#[async_trait]
impl VisualPerceptionProvider for HttpVlmProvider {
    async fn analyze_image(
        &self,
        image_data: &[u8],
        prompt: &str,
    ) -> Result<VisualAnalysisResult, VisualError> {
        // Until real screenshot capture is wired in, callers may pass a
        // simulated visual state that is not a valid PNG. Do not waste a
        // paid VLM call on placeholder bytes; return a simulated analysis.
        if !is_png(image_data) {
            return Ok(VisualAnalysisResult {
                description: format!("Simulated analysis for prompt: {}", prompt),
                detected_elements: vec![],
                confidence: 0.9,
            });
        }
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

        let response = self
            .client
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
            return Err(VisualError::ProviderError(format!(
                "HTTP {}: {}",
                status, body
            )));
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
            // FIXME: parse structured elements from the OCR text. Issue #visual-structured-output.
            // Currently returns an empty list and relies on the free-form `description`.
            detected_elements: vec![],
            confidence: 0.9,
        })
    }

    async fn get_embedding(&self, image_data: &[u8]) -> Result<Vec<f32>, VisualError> {
        // Same guard as analyze_image: placeholder bytes must not be sent to
        // the paid embedding endpoint. Return the mock vector instead.
        if !is_png(image_data) {
            return Ok(vec![0.1; 512]);
        }
        let base64_image = base64_encode(image_data);

        let embeddings_endpoint = self
            .config
            .endpoint
            .replace("/chat/completions", "/embeddings");

        let request_body = serde_json::json!({
            "model": self.config.model,
            "input": format!("data:image/png;base64,{}", base64_image),
        });

        let response = self
            .client
            .post(&embeddings_endpoint)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| VisualError::HttpError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(VisualError::ProviderError(format!(
                "Embedding HTTP {}: {}",
                status, body
            )));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| VisualError::HttpError(e.to_string()))?;

        let embedding = response_json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| VisualError::ProviderError("No embedding array in response".into()))?;

        let vector: Vec<f32> = embedding
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        if vector.is_empty() {
            return Err(VisualError::ProviderError("Empty embedding vector".into()));
        }

        Ok(vector)
    }
}

// ── Mock provider (for testing) ─────────────────────────────────────────

/// Sisyphus Visual Provider.
pub struct SisyphusVisualProvider {
    _llm_client: Arc<dyn LlmClient>,
}

impl SisyphusVisualProvider {
    /// New.
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self {
            _llm_client: llm_client,
        }
    }
}

#[async_trait]
impl VisualPerceptionProvider for SisyphusVisualProvider {
    async fn analyze_image(
        &self,
        image_data: &[u8],
        prompt: &str,
    ) -> Result<VisualAnalysisResult, VisualError> {
        tracing::info!(
            "VLM analyzing image ({} bytes) with prompt: {}",
            image_data.len(),
            prompt
        );
        Ok(VisualAnalysisResult {
            description: format!("Simulated analysis for prompt: {}", prompt),
            detected_elements: vec![],
            confidence: 0.9,
        })
    }

    async fn get_embedding(&self, _image_data: &[u8]) -> Result<Vec<f32>, VisualError> {
        Ok(vec![0.1; 512])
    }
}

// ── VisualPerceptionManager ──────────────────────────────────────────────

/// Visual Perception Manager.
pub struct VisualPerceptionManager {
    provider: Arc<dyn VisualPerceptionProvider>,
    _llm_client: Arc<dyn LlmClient>,
}

impl VisualPerceptionManager {
    /// New.
    pub fn new(
        provider: Arc<dyn VisualPerceptionProvider>,
        llm_client: Arc<dyn LlmClient>,
    ) -> Self {
        Self {
            provider,
            _llm_client: llm_client,
        }
    }

    /// Processes a visual request based on the NodeKind.
    /// Process visual node.
    pub async fn process_visual_node(
        &self,
        kind: &NodeKind,
        image_data: &[u8],
    ) -> Result<String, VisualError> {
        counter!("sisyphus_visual_nodes_processed_total").increment(1);
        match kind {
            NodeKind::VisualAnalysis { prompt, .. } => {
                let result = self.provider.analyze_image(image_data, prompt).await?;
                Ok(serde_json::to_string(&result)
                    .map_err(|e| VisualError::ProcessingError(e.to_string()))?)
            }
            NodeKind::VisualAction {
                action_type,
                target_selector,
                params,
            } => {
                let prompt = format!(
                    "Locate the element matching selector '{}' for action '{}' with params {}. Return coordinates.",
                    target_selector, action_type, params
                );
                let result = self.provider.analyze_image(image_data, &prompt).await?;
                Ok(serde_json::to_string(&result)
                    .map_err(|e| VisualError::ProcessingError(e.to_string()))?)
            }
            _ => Err(VisualError::InvalidRequest(
                "Node is not a visual node".into(),
            )),
        }
    }

    /// Get embedding.
    pub async fn get_embedding(&self, image_data: &[u8]) -> Result<Vec<f32>, VisualError> {
        self.provider.get_embedding(image_data).await
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

pub mod helpers {
    /// PNG file signature bytes.
    pub const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    /// Returns true when the bytes start with a valid PNG signature.
    pub fn is_png(data: &[u8]) -> bool {
        data.len() >= PNG_MAGIC.len() && data[..PNG_MAGIC.len()] == PNG_MAGIC
    }

    /// Builds a deterministic placeholder image (valid PNG header) derived from a
    /// seed string, so different goals produce different visual states.
    ///
    /// Used to simulate a captured visual state until real screenshot capture is
    /// wired in. The deterministic bytes keep embedding lookups stable across
    /// runs, and the PNG header prevents simulated states from being mistaken
    /// for corrupt data by providers.
    pub fn simulated_visual_state(seed: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(1024);
        out.extend_from_slice(&PNG_MAGIC);
        let mut h: u32 = 2_166_136_261; // FNV-1a offset basis
        for b in seed.bytes() {
            h ^= u32::from(b);
            h = h.wrapping_mul(16_777_619);
        }
        for i in 0u32..1016 {
            h ^= i;
            h = h.wrapping_mul(16_777_619);
            out.push((h >> 24) as u8);
        }
        out
    }

    /// Simple base64 encoding (no dependency needed).
    pub fn base64_encode(data: &[u8]) -> String {
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
        let result = provider
            .analyze_image(b"test image", "describe this")
            .await
            .unwrap();
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
    fn test_simulated_visual_state_is_deterministic_png() {
        let a = simulated_visual_state("fetch and process");
        let b = simulated_visual_state("fetch and process");
        let c = simulated_visual_state("different goal");
        assert_eq!(a.len(), 1024);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(is_png(&a));
    }

    #[tokio::test]
    async fn test_http_provider_rejects_placeholder_bytes() {
        let config = VlmProviderConfig {
            api_key: "test-key".into(),
            endpoint: "https://api.example.com/v1/vision".into(),
            model: "test-model".into(),
            ..Default::default()
        };
        let provider = HttpVlmProvider::new(config).unwrap();
        // Non-PNG placeholder bytes must short-circuit without any HTTP call.
        let embedding = provider.get_embedding(&[0u8; 64]).await.unwrap();
        assert_eq!(embedding.len(), 512);
        let analysis = provider
            .analyze_image(&[0u8; 64], "describe")
            .await
            .unwrap();
        assert!(analysis.description.contains("Simulated"));
    }

    #[test]
    fn test_http_provider_creation() {
        let config = VlmProviderConfig {
            api_key: "test-key".into(),
            endpoint: "https://api.example.com/v1/vision".into(),
            model: "test-model".into(),
            ..Default::default()
        };
        let provider = HttpVlmProvider::new(config).unwrap();
        // Just verify it doesn't panic
        assert!(format!("{:?}", provider).contains("HttpVlmProvider"));
    }
}
