//! SDK data models.

use serde::{Deserialize, Serialize};

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// State response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateResponse {
    pub key: String,
    pub value: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

/// State set request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetStateRequest {
    pub value: serde_json::Value,
}

/// State list response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListStateResponse {
    pub items: Vec<StateResponse>,
    pub total: i64,
}

/// Vector search request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchRequest {
    pub query: Vec<f32>,
    pub top_k: Option<usize>,
    pub filter: Option<serde_json::Value>,
}

/// Vector search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchResult {
    pub key: String,
    pub score: f32,
    pub metadata: Option<serde_json::Value>,
}

/// Vector search response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchResponse {
    pub results: Vec<VectorSearchResult>,
}
