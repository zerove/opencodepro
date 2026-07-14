use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceTuple {
    pub context_text: String,
    pub context_hash: String,
    pub action_id: String,
    pub project_id: Option<String>,
    pub outcome_success: bool,
    pub deviation_score: f64,
    pub cost: u64,
    pub timestamp: u64,
    pub embedding: Option<Vec<f32>>,
    pub visual_embedding: Option<Vec<f32>>,
    pub insight: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceQuery {
    pub query_text: String,
    pub context_hash: String,
    pub project_id: Option<String>,
    pub action_id: Option<String>,
    pub visual_embedding: Option<Vec<f32>>,
    pub limit: usize,
}

#[async_trait]
pub trait ExperienceBank: Send + Sync {
    async fn record(&self, exp: ExperienceTuple) -> Result<(), String>;
    async fn query_similar(&self, query: ExperienceQuery) -> Result<Vec<ExperienceTuple>, String>;
}
