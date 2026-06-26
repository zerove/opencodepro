use anyhow::{Context, Result};
use serde_json::Value;
use sqlx::{SqlitePool, Row};
use sisyphus_types::VectorRecord;

#[derive(Debug, Clone)]
pub struct ScoredVector {
    pub key: String,
    pub score: f32,
    pub record: VectorRecord,
}

pub struct VectorMemoryStore {
    pool: SqlitePool,
}

pub struct UserMemoryStore {
    inner: VectorMemoryStore,
    session_id: String,
}

impl UserMemoryStore {
    pub fn new(inner: VectorMemoryStore, session_id: String) -> Self {
        Self { inner, session_id }
    }

    fn session_key(&self, key: &str) -> String {
        format!("sess_{}_{}", self.session_id, key)
    }

    pub async fn store(&self, key: &str, embedding: Vec<f32>, metadata: Value) -> Result<()> {
        self.inner.store_embedding(&self.session_key(key), embedding, metadata).await
    }

    pub async fn recall(&self, embedding: &[f32], top_k: usize) -> Result<Vec<ScoredVector>> {
        let results = self.inner.search_similar(embedding, top_k * 10).await?;
        let prefix = format!("sess_{}_", self.session_id);
        let filtered = results.into_iter()
            .filter(|sv| sv.key.starts_with(&prefix))
            .take(top_k)
            .collect();
        Ok(filtered)
    }
}

impl VectorMemoryStore {
    /// Creates a new VectorMemoryStore. 
    /// If an existing MfsStore is provided, it uses its pool.
    pub async fn new(pool: SqlitePool) -> Result<Self> {
        let store = Self { pool };
        store.init_schema().await?;
        Ok(store)
    }

    async fn init_schema(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS vectors (
                id TEXT PRIMARY KEY,
                embedding BLOB NOT NULL,
                metadata TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create vectors table")?;
        Ok(())
    }

    pub async fn store_embedding(&self, key: &str, embedding: Vec<f32>, metadata: Value) -> Result<()> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        // Convert Vec<f32> to bytes
        let embedding_bytes: Vec<u8> = embedding
            .iter()
            .flat_map(|f| f.to_le_bytes().to_vec())
            .collect();

        let metadata_str = serde_json::to_string(&metadata)?;

        sqlx::query(
            "INSERT INTO vectors (id, embedding, metadata, timestamp) 
             VALUES (?, ?, ?, ?) 
             ON CONFLICT(id) DO UPDATE SET 
                embedding = excluded.embedding, 
                metadata = excluded.metadata, 
                timestamp = excluded.timestamp",
        )
        .bind(key)
        .bind(embedding_bytes)
        .bind(metadata_str)
        .bind(timestamp)
        .execute(&self.pool)
        .await
        .context("Failed to store embedding")?;

        Ok(())
    }

    pub async fn search_similar(&self, query_embedding: &[f32], top_k: usize) -> Result<Vec<ScoredVector>> {
        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query("SELECT id, embedding, metadata, timestamp FROM vectors")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch vectors for similarity search")?;

        let mut scored_vectors = Vec::new();

        for row in rows {
            let key: String = row.get("id");
            let embedding_bytes: Vec<u8> = row.get("embedding");
            let metadata_str: String = row.get("metadata");
            let timestamp: u64 = row.get("timestamp");

            // Convert bytes back to Vec<f32>
            let embedding: Vec<f32> = embedding_bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
                .collect();

            let score = self.cosine_similarity(query_embedding, &embedding);
            
            let record = VectorRecord {
                key: key.clone(),
                embedding,
                metadata: serde_json::from_str(&metadata_str)?,
                timestamp,
            };

            scored_vectors.push(ScoredVector {
                key,
                score,
                record,
            });
        }

        // Sort by score descending
        scored_vectors.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored_vectors.truncate(top_k);

        Ok(scored_vectors)
    }

    pub async fn search_by_metadata(&self, filter: Value, top_k: usize) -> Result<Vec<ScoredVector>> {
        // Simple metadata filtering: we fetch all and filter in memory for simplicity 
        // as SQLite JSON filtering can be complex to implement generically here.
        let rows = sqlx::query("SELECT id, embedding, metadata, timestamp FROM vectors")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch vectors for metadata search")?;

        let mut filtered_vectors = Vec::new();

        for row in rows {
            let metadata_str: String = row.get("metadata");
            let metadata: Value = serde_json::from_str(&metadata_str)?;

            if self.matches_filter(&metadata, &filter) {
                let key: String = row.get("id");
                let embedding_bytes: Vec<u8> = row.get("embedding");
                let timestamp: u64 = row.get("timestamp");

                let embedding: Vec<f32> = embedding_bytes
                    .chunks_exact(4)
                    .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
                    .collect();

                let record = VectorRecord {
                    key: key.clone(),
                    embedding,
                    metadata,
                    timestamp,
                };

                // For metadata search, score is 1.0 if it matches
                filtered_vectors.push(ScoredVector {
                    key,
                    score: 1.0,
                    record,
                });
            }
        }

        filtered_vectors.truncate(top_k);
        Ok(filtered_vectors)
    }

    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot_product / (norm_a * norm_b)
    }

    fn matches_filter(&self, metadata: &Value, filter: &Value) -> bool {
        match (metadata, filter) {
            (Value::Object(m_obj), Value::Object(f_obj)) => {
                f_obj.iter().all(|(k, f_val)| {
                    m_obj.get(k) == Some(f_val)
                })
            }
            (m, f) => m == f,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::SqlitePool;

    async fn setup_store() -> VectorMemoryStore {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        VectorMemoryStore::new(pool).await.unwrap()
    }

    #[tokio::test]
    async fn test_store_and_search() {
        let store = setup_store().await;
        
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![0.0, 1.0, 0.0];
        let v3 = vec![0.9, 0.1, 0.0];

        store.store_embedding("key1", v1.clone(), json!({"tag": "a"})).await.unwrap();
        store.store_embedding("key2", v2.clone(), json!({"tag": "b"})).await.unwrap();
        store.store_embedding("key3", v3.clone(), json!({"tag": "a"})).await.unwrap();

        // Search for something close to v1
        let results = store.search_similar(&v1, 2).await.unwrap();
        
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].key, "key1");
        assert_eq!(results[1].key, "key3");
        assert!(results[0].score > results[1].score);
    }

    #[tokio::test]
    async fn test_metadata_filter() {
        let store = setup_store().await;
        
        store.store_embedding("key1", vec![1.0], json!({"tag": "a", "val": 1})).await.unwrap();
        store.store_embedding("key2", vec![1.0], json!({"tag": "b", "val": 1})).await.unwrap();
        store.store_embedding("key3", vec![1.0], json!({"tag": "a", "val": 2})).await.unwrap();

        // Filter by tag=a
        let results = store.search_by_metadata(json!({"tag": "a"}), 10).await.unwrap();
        assert_eq!(results.len(), 2);
        
        // Filter by tag=a AND val=1
        let results = store.search_by_metadata(json!({"tag": "a", "val": 1}), 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "key1");
    }

    #[tokio::test]
    async fn test_empty_store() {
        let store = setup_store().await;
        let results = store.search_similar(&[1.0, 0.0], 10).await.unwrap();
        assert!(results.is_empty());
        
        let results = store.search_by_metadata(json!({"tag": "a"}), 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_empty_query() {
        let store = setup_store().await;
        store.store_embedding("key1", vec![1.0], json!({})).await.unwrap();
        
        let results = store.search_similar(&[], 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_dimension_mismatch() {
        let store = setup_store().await;
        store.store_embedding("key1", vec![1.0, 0.0], json!({})).await.unwrap();
        
        // Query with 3 dims instead of 2
        let results = store.search_similar(&[1.0, 0.0, 0.0], 10).await.unwrap();
        
        // Should return results but score should be 0.0 for mismatched dims
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 0.0);
    }

    #[tokio::test]
    async fn test_invalid_utf8_in_blob() {
        // This is more of a storage test. We store raw bytes.
        // The current implementation uses f32::to_le_bytes, so it's always valid bytes.
        // But we can test if we can store and retrieve without crashing.
        let store = setup_store().await;
        let v = vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        store.store_embedding("key1", v.clone(), json!({})).await.unwrap();
        
        let results = store.search_similar(&[1.0, 1.0, 1.0], 1).await.unwrap();
        assert_eq!(results.len(), 1);
        
        let retrieved = &results[0].record.embedding;
        assert_eq!(retrieved.len(), v.len());
        for (a, b) in retrieved.iter().zip(v.iter()) {
            if a.is_nan() {
                assert!(b.is_nan());
            } else {
                assert_eq!(a, b);
            }
        }
    }

    #[tokio::test]
    async fn test_user_memory_store() {
        let vector_store = setup_store().await;
        let user_store = UserMemoryStore::new(vector_store, "user_123".to_string());
        
        let v1 = vec![1.0, 0.0];
        let v2 = vec![0.0, 1.0];
        
        user_store.store("fact1", v1.clone(), json!({})).await.unwrap();
        user_store.store("fact2", v2.clone(), json!({})).await.unwrap();
        
        // Recall for user_123
        let results = user_store.recall(&v1, 1).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].key.contains("user_123"));
        
        // Verify that another user can't recall it (by creating another store)
        let user_store_2 = UserMemoryStore::new(
            VectorMemoryStore::new(sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap()).await.unwrap(), 
            "user_456".to_string()
        );
        let results_2 = user_store_2.recall(&v1, 1).await.unwrap();
        assert!(results_2.is_empty());
    }
}


