//! API data models with OpenAPI schema generation.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Health check response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// State get response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StateResponse {
    pub key: String,
    pub value: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

/// State set request
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SetStateRequest {
    pub value: serde_json::Value,
}

/// State list response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListStateResponse {
    pub items: Vec<StateResponse>,
    pub total: i64,
}

/// Vector search request
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VectorSearchRequest {
    pub query: Vec<f32>,
    pub top_k: Option<usize>,
    pub filter: Option<serde_json::Value>,
}

/// Vector store request
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StoreVectorRequest {
    pub key: String,
    pub embedding: Vec<f32>,
    pub metadata: Option<serde_json::Value>,
}

/// Vector search result
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VectorSearchResult {
    pub key: String,
    pub score: f32,
    pub metadata: Option<serde_json::Value>,
}

/// Vector search response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VectorSearchResponse {
    pub results: Vec<VectorSearchResult>,
}

/// Vector stats response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VectorStatsResponse {
    pub total_vectors: i64,
    pub dimension: usize,
}

/// Memory store request
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StoreMemoryRequest {
    pub key: String,
    pub value: serde_json::Value,
    pub embedding: Option<Vec<f32>>,
    pub metadata: Option<serde_json::Value>,
}

/// Memory search request
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SearchMemoryRequest {
    pub query: String,
    pub embedding: Option<Vec<f32>>,
    pub top_k: Option<usize>,
}

/// Experience record
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ExperienceRecord {
    pub id: String,
    pub sop_id: String,
    pub task_desc: String,
    pub success: bool,
    pub metrics: Option<serde_json::Value>,
    pub created_at: i64,
}

/// Store experience request
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StoreExperienceRequest {
    pub sop_id: String,
    pub task_desc: String,
    pub success: bool,
    pub metrics: Option<serde_json::Value>,
}

/// Error response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}
