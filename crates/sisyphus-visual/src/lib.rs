//! Sisyphus Visual Perception system for multi-modal cognitive synergy.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use sisyphus_types::graph::NodeKind;
use sisyphus_tools_llm::LlmClient;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VisualError {
    #[error("VLM provider error: {0}")]
    ProviderError(String),
    #[error("Image processing error: {0}")]
    ProcessingError(String),
    #[error("Invalid visual request: {0}")]
    InvalidRequest(String),
}

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

pub struct VisualPerceptionManager {
    provider: Arc<dyn VisualPerceptionProvider>,
    llm_client: Arc<dyn LlmClient>,
}

impl VisualPerceptionManager {
    pub fn new(provider: Arc<dyn VisualPerceptionProvider>, llm_client: Arc<dyn LlmClient>) -> Self {
        Self { provider, llm_client }
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
                // For VisualAction, we first analyze the image to find the target,
                // then we return the precise coordinates/action for the executor.
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

pub struct SisyphusVisualProvider {
    llm_client: Arc<dyn LlmClient>,
}

impl SisyphusVisualProvider {
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self { llm_client }
    }
}

#[async_trait]
impl VisualPerceptionProvider for SisyphusVisualProvider {
    async fn analyze_image(
        &self, 
        image_data: &[u8], 
        prompt: &str
    ) -> Result<VisualAnalysisResult, VisualError> {
        tracing::info!("VLM analyzing image with prompt: {}", prompt);
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
