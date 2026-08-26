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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response_roundtrip() {
        let h = HealthResponse {
            status: "ok".into(),
            version: "0.1.0".into(),
        };
        let json = serde_json::to_string(&h).unwrap();
        let back: HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, "ok");
        assert_eq!(back.version, "0.1.0");
    }

    #[test]
    fn test_state_response_roundtrip() {
        let s = StateResponse {
            key: "k1".into(),
            value: serde_json::json!({"a": 1}),
            created_at: 100,
            updated_at: 200,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: StateResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.key, "k1");
        assert_eq!(back.created_at, 100);
        assert!(back.value.is_object());
    }

    #[test]
    fn test_vector_search_request_roundtrip() {
        let v = VectorSearchRequest {
            query: vec![0.1, 0.2, 0.3],
            top_k: Some(5),
            filter: None,
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: VectorSearchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.query.len(), 3);
        assert_eq!(back.top_k, Some(5));
    }

    #[test]
    fn test_experience_record_roundtrip() {
        let e = ExperienceRecord {
            id: "exp1".into(),
            sop_id: "sop1".into(),
            task_desc: "do something".into(),
            success: true,
            metrics: Some(serde_json::json!({"fuel": 100})),
            created_at: 123456,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ExperienceRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "exp1");
        assert!(back.success);
        assert_eq!(back.created_at, 123456);
    }

    #[test]
    fn test_error_response_roundtrip() {
        let e = ErrorResponse {
            error: "not_found".into(),
            message: "item missing".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.error, "not_found");
        assert_eq!(back.message, "item missing");
    }

    #[test]
    fn test_vector_stats_response_roundtrip() {
        let v = VectorStatsResponse {
            total_vectors: 42,
            dimension: 768,
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: VectorStatsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_vectors, 42);
        assert_eq!(back.dimension, 768);
    }
}
