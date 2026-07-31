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
    }

    #[test]
    fn test_state_response_roundtrip() {
        let s = StateResponse {
            key: "k".into(),
            value: serde_json::json!({"x": 1}),
            created_at: 1,
            updated_at: 2,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: StateResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.key, "k");
    }

    #[test]
    fn test_vector_search_response_roundtrip() {
        let v = VectorSearchResponse {
            results: vec![VectorSearchResult {
                key: "k".into(),
                score: 0.99,
                metadata: None,
            }],
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: VectorSearchResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.results.len(), 1);
        assert_eq!(back.results[0].score, 0.99);
    }
}
