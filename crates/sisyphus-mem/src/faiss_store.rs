//! Vector store with brute-force similarity search.
//! Uses SQLite for persistence and in-memory brute-force for k-NN queries.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sisyphus_types::VectorRecord;
use sqlx::{Pool, Row, Sqlite};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredVector {
    pub key: String,
    pub score: f32,
    pub record: VectorRecord,
}

/// Vector store with SQLite persistence and brute-force similarity search
pub struct VectorStore {
    pool: Pool<Sqlite>,
    dimension: usize,
}

impl VectorStore {
    /// Creates a new vector store.
    pub async fn new(pool: Pool<Sqlite>, dimension: usize) -> Result<Self> {
        Self::init_schema(&pool).await?;
        Ok(Self { pool, dimension })
    }

    async fn init_schema(pool: &Pool<Sqlite>) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS vectors (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key TEXT UNIQUE NOT NULL,
                project_id TEXT DEFAULT 'default',
                collection TEXT NOT NULL DEFAULT 'default',
                embedding BLOB NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            );
            
            CREATE INDEX IF NOT EXISTS idx_vectors_project ON vectors(project_id);
            CREATE INDEX IF NOT EXISTS idx_vectors_collection ON vectors(collection);
            "#,
        )
        .execute(pool)
        .await
        .context("Failed to create vectors table")?;
        Ok(())
    }

    /// Store an embedding
    pub async fn store_embedding(
        &self,
        key: &str,
        embedding: &[f32],
        metadata: &Value,
    ) -> Result<()> {
        if embedding.len() != self.dimension {
            anyhow::bail!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.len()
            );
        }

        let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        let metadata_str = serde_json::to_string(metadata)?;

        sqlx::query(
            r#"INSERT INTO vectors (key, project_id, collection, embedding, metadata) 
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(key) DO UPDATE SET embedding = excluded.embedding, metadata = excluded.metadata"#
        )
        .bind(key)
        .bind(metadata.get("project_id").and_then(|v| v.as_str()).unwrap_or("default"))
        .bind(metadata.get("collection").and_then(|v| v.as_str()).unwrap_or("default"))
        .bind(&embedding_bytes)
        .bind(&metadata_str)
        .execute(&self.pool)
        .await
        .context("Failed to store embedding")?;
        Ok(())
    }

    /// Search for similar vectors
    pub async fn search_similar(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<ScoredVector>> {
        if query_embedding.len() != self.dimension {
            anyhow::bail!("Query embedding dimension mismatch");
        }

        if top_k == 0 {
            return Ok(Vec::new());
        }

        let rows = sqlx::query("SELECT key, embedding, metadata, created_at FROM vectors")
            .fetch_all(&self.pool)
            .await?;

        let mut scored = Vec::new();
        for row in rows {
            let key: String = row.get("key");
            let embedding_bytes: Vec<u8> = row.get("embedding");
            let metadata_str: String = row.get("metadata");
            let timestamp: i64 = row.get("created_at");

            let embedding: Vec<f32> = embedding_bytes
                .chunks_exact(4)
                .map(|chunk| {
                    let arr: [u8; 4] = chunk.try_into().unwrap_or([0, 0, 0, 0]);
                    f32::from_le_bytes(arr)
                })
                .collect();

            if embedding.len() == self.dimension {
                let score = cosine_similarity(&embedding, query_embedding);
                let metadata: Value = serde_json::from_str(&metadata_str).unwrap_or(Value::Null);

                scored.push(ScoredVector {
                    key: key.clone(),
                    score,
                    record: VectorRecord {
                        key,
                        embedding,
                        metadata,
                        timestamp: timestamp as u64,
                    },
                });
            }
        }

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(top_k);
        Ok(scored)
    }

    /// Search by metadata filter
    pub async fn search_by_metadata(
        &self,
        filter: &Value,
        top_k: usize,
    ) -> Result<Vec<ScoredVector>> {
        let rows = sqlx::query(
            "SELECT key, embedding, metadata, created_at FROM vectors WHERE project_id = ? AND collection = ?"
        )
        .bind(filter.get("project_id").and_then(|v| v.as_str()).unwrap_or("default"))
        .bind(filter.get("collection").and_then(|v| v.as_str()).unwrap_or("default"))
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch vectors for metadata search")?;

        let mut results = Vec::new();
        for row in rows {
            let metadata_str: String = row.get("metadata");
            let metadata: Value = serde_json::from_str(&metadata_str).unwrap_or(Value::Null);

            if matches_filter(&metadata, filter) {
                let key: String = row.get("key");
                let embedding_bytes: Vec<u8> = row.get("embedding");
                let timestamp: i64 = row.get("created_at");

                let embedding: Vec<f32> = embedding_bytes
                    .chunks_exact(4)
                    .map(|chunk| {
                        let arr: [u8; 4] = chunk.try_into().unwrap_or([0, 0, 0, 0]);
                        f32::from_le_bytes(arr)
                    })
                    .collect();

                results.push(ScoredVector {
                    key: key.clone(),
                    score: 1.0,
                    record: VectorRecord {
                        key,
                        embedding,
                        metadata,
                        timestamp: timestamp as u64,
                    },
                });
            }
        }

        results.truncate(top_k);
        Ok(results)
    }

    /// Get total count of vectors
    pub async fn count(&self) -> Result<i64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM vectors")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    /// Delete a vector by key
    pub async fn delete(&self, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM vectors WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await
            .context("Failed to delete vector")?;
        Ok(())
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a.sqrt() * norm_b.sqrt())
    }
}

fn matches_filter(metadata: &Value, filter: &Value) -> bool {
    match (metadata, filter) {
        (Value::Object(m_obj), Value::Object(f_obj)) => {
            f_obj.iter().all(|(k, f_val)| m_obj.get(k) == Some(f_val))
        }
        (m, f) => m == f,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn test_store_and_search() -> Result<()> {
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await?;
        let store = VectorStore::new(pool, 3).await?;

        // Store some vectors
        store
            .store_embedding(
                "v1",
                &[1.0, 0.0, 0.0],
                &serde_json::json!({"project_id": "p1", "collection": "c1"}),
            )
            .await?;

        store
            .store_embedding(
                "v2",
                &[0.0, 1.0, 0.0],
                &serde_json::json!({"project_id": "p1", "collection": "c1"}),
            )
            .await?;

        // Search
        let results = store.search_similar(&[1.0, 0.0, 0.0], 2).await?;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].key, "v1");
        assert!(results[0].score > results[1].score);

        Ok(())
    }
}
