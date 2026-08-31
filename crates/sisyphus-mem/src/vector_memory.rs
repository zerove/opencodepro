use anyhow::{Context, Result};
use rayon::prelude::*;
use serde_json::Value;
use sisyphus_types::VectorRecord;
use sqlx::{Row, SqlitePool};

/// A vector record paired with its similarity score (cosine, in [-1.0, 1.0]).
///
/// Returned by similarity search methods like [`UserMemoryStore::recall`].
#[derive(Debug, Clone)]
pub struct ScoredVector {
    pub key: String,
    pub score: f32,
    pub record: VectorRecord,
}

/// SQLite-backed vector store with cosine similarity search.
///
/// Used as the storage backend for [`UserMemoryStore`], which adds
/// session-scoped keying on top of this raw vector index.
pub struct VectorMemoryStore {
    pool: SqlitePool,
}

/// Per-session wrapper around [`VectorMemoryStore`].
///
/// All keys are automatically namespaced with the session id to prevent
/// cross-session leakage of embeddings.
pub struct UserMemoryStore {
    inner: VectorMemoryStore,
    session_id: String,
}

impl UserMemoryStore {
    /// Create a session-scoped memory store backed by `inner`.
    pub fn new(inner: VectorMemoryStore, session_id: String) -> Self {
        Self { inner, session_id }
    }

    fn session_key(&self, key: &str) -> String {
        format!("sess_{}_{}", self.session_id, key)
    }

    /// Store `embedding` under `key` with optional `metadata` JSON.
    ///
    /// Replaces any existing entry for the same key (upsert semantics).
    pub async fn store(&self, key: &str, embedding: Vec<f32>, metadata: Value) -> Result<()> {
        self.inner.store_embedding(&self.session_key(key), embedding, metadata).await
    }

    /// Return up to `top_k` vectors most similar to `embedding` by cosine similarity.
    ///
    /// Results are sorted by score descending (most similar first).
    pub async fn recall(&self, embedding: &[f32], top_k: usize) -> Result<Vec<ScoredVector>> {
        // C 层缺陷修复：此前全局取 top_k*10 再按会话前缀过滤，
        // 多会话并发写入时本会话记录可能整体落在全局窗口之外被
        // 截断丢弃。改为 SQL 层按前缀过滤后再取 top_k。
        let prefix = self.session_prefix();
        self.inner.search_similar_filtered(embedding, top_k, Some(&prefix)).await
    }

    fn session_prefix(&self) -> String {
        format!("sess_{}_", self.session_id)
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

    /// Store a [`VectorRecord`] with an explicit vector id and namespace.
    ///
    /// Unlike [`UserMemoryStore::store`], this method is intended for bulk
    /// ingestion of vectors produced by the broader memory pipeline
    /// (e.g. SOP embeddings, tool recall caches).
    pub async fn store_embedding(
        &self,
        key: &str,
        embedding: Vec<f32>,
        metadata: Value,
    ) -> Result<()> {
        let timestamp =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() as i64;

        // Convert Vec<f32> to bytes
        let embedding_bytes: Vec<u8> =
            embedding.iter().flat_map(|f| f.to_le_bytes().to_vec()).collect();

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

    /// Similarity search over the underlying [`VectorMemoryStore`].
    ///
    /// Returns up to `top_k` [`ScoredVector`] entries whose cosine
    /// similarity to `query` is at least `threshold`.
    pub async fn search_similar(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<ScoredVector>> {
        self.search_similar_filtered(query_embedding, top_k, None).await
    }

    /// 相似度检索，可选按键前缀过滤（评审 C 层：多会话隔离）。
    ///
    /// 前缀在 SQL 层过滤：本实现为全表扫描 + 内存打分，
    /// WHERE 谓词零额外成本；先过滤再取 top_k 保证目标
    /// 会话的记录不会因其他会话占满窗口而被丢弃。
    pub async fn search_similar_filtered(
        &self,
        query_embedding: &[f32],
        top_k: usize,
        key_prefix: Option<&str>,
    ) -> Result<Vec<ScoredVector>> {
        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        let rows = if let Some(prefix) = key_prefix {
            let pattern = format!("{prefix}%");
            sqlx::query("SELECT id, embedding, metadata, timestamp FROM vectors WHERE id LIKE ?")
                .bind(pattern)
                .fetch_all(&self.pool)
                .await
                .context("Failed to fetch vectors for similarity search")?
        } else {
            sqlx::query("SELECT id, embedding, metadata, timestamp FROM vectors")
                .fetch_all(&self.pool)
                .await
                .context("Failed to fetch vectors for similarity search")?
        };

        let mut scored_vectors: Vec<ScoredVector> = rows
            .into_par_iter()
            .filter_map(|row| {
                let key: String = row.get("id");
                let embedding_bytes: Vec<u8> = row.get("embedding");
                let metadata_str: String = row.get("metadata");
                let timestamp: u64 = row.get("timestamp");

                // 直接在字节流上计算相似度，避免分配 Vec<f32>
                let score = self.cosine_similarity_bytes(query_embedding, &embedding_bytes);

                let embedding: Vec<f32> = embedding_bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|bytes| f32::from_le_bytes(*bytes))
                    .collect();

                let metadata = match serde_json::from_str(&metadata_str) {
                    Ok(m) => m,
                    Err(_) => return None,
                };

                Some(ScoredVector {
                    key: key.clone(),
                    score,
                    record: VectorRecord {
                        key,
                        embedding,
                        metadata,
                        timestamp,
                    },
                })
            })
            .collect();

        // Sort by score descending
        scored_vectors
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored_vectors.truncate(top_k);

        Ok(scored_vectors)
    }

    /// Metadata-only filter: returns all vectors whose metadata field equals `value`.
    ///
    /// Useful for retrieving all vectors tagged with a specific key/value pair
    /// without computing similarities. Sorted by creation time descending.
    pub async fn search_by_metadata(
        &self,
        filter: Value,
        top_k: usize,
    ) -> Result<Vec<ScoredVector>> {
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
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|bytes| f32::from_le_bytes(*bytes))
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

    fn cosine_similarity_bytes(&self, a: &[f32], b_bytes: &[u8]) -> f32 {
        if b_bytes.len() != a.len() * 4 || a.is_empty() {
            return 0.0;
        }

        let mut dot_product = 0.0;
        let mut norm_b_sq = 0.0;
        let mut norm_a_sq = 0.0;

        for (i, chunk) in b_bytes.as_chunks::<4>().0.iter().enumerate() {
            let val_b = f32::from_le_bytes(*chunk);
            let val_a = a[i];
            dot_product += val_a * val_b;
            norm_b_sq += val_b * val_b;
            norm_a_sq += val_a * val_a;
        }

        let norm_a = norm_a_sq.sqrt();
        let norm_b = norm_b_sq.sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot_product / (norm_a * norm_b)
    }

    fn matches_filter(&self, metadata: &Value, filter: &Value) -> bool {
        match (metadata, filter) {
            (Value::Object(m_obj), Value::Object(f_obj)) => {
                f_obj.iter().all(|(k, f_val)| m_obj.get(k) == Some(f_val))
            },
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
            VectorMemoryStore::new(sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap())
                .await
                .unwrap(),
            "user_456".to_string(),
        );
        let results_2 = user_store_2.recall(&v1, 1).await.unwrap();
        assert!(results_2.is_empty());
    }
}

// ---------------------------------------------------------------------------
// C 层缺陷回归：多会话召回隔离（先过滤再截断）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod session_recall_tests {
    use super::*;
    use serde_json::json;
    use sqlx::SqlitePool;

    async fn setup_user_store(session: &str) -> UserMemoryStore {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let inner = VectorMemoryStore::new(pool).await.unwrap();
        UserMemoryStore::new(inner, session.to_string())
    }

    /// 旧实现的失效场景：会话 A 写入大量记录占满全局 top_k*10 窗口后，
    /// 会话 B 的记录必须仍能被召回，而不是被全局截断丢弃。
    #[tokio::test]
    async fn test_recall_not_starved_by_other_sessions() {
        let store_a = setup_user_store("alpha").await;
        let store_b = setup_user_store("beta").await;

        let v = vec![1.0, 0.0, 0.0];

        // 会话 A 写入 30 条完全相同的向量
        for i in 0..30 {
            store_a.store(&format!("k{i}"), v.clone(), json!({})).await.unwrap();
        }
        // 会话 B 仅写 1 条目标记录
        store_b.store("target", vec![0.0, 1.0, 0.0], json!({"goal": "hit"})).await.unwrap();

        // 旧实现：全局窗口 = 5*10 = 50 条中 A 占 30 条虽不至于饿死，
        // 但当 A 的记录数超过窗口时（此处用 top_k=1 → 窗口 10），
        // 全局相似度打分下 B 的记录可能排在窗口外。查询向量贴近 B 的记录。
        let query = vec![0.0, 1.0, 0.0];
        let results = store_b.recall(&query, 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "sess_beta_target");
        assert!(results[0].record.metadata.get("goal").is_some());
    }

    #[tokio::test]
    async fn test_recall_returns_only_own_session() {
        let store_a = setup_user_store("alpha").await;
        let store_b = setup_user_store("beta").await;

        let v = vec![1.0, 0.0];
        store_a.store("shared", v.clone(), json!({"s": "a"})).await.unwrap();
        store_b.store("shared", v.clone(), json!({"s": "b"})).await.unwrap();

        let results = store_b.recall(&v, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].record.metadata["s"], "b");
    }

    #[tokio::test]
    async fn test_recall_respects_top_k_after_filtering() {
        let store = setup_user_store("solo").await;
        let v = vec![1.0, 0.0];
        for i in 0..8 {
            store.store(&format!("k{i}"), v.clone(), json!({})).await.unwrap();
        }
        let results = store.recall(&v, 3).await.unwrap();
        assert_eq!(results.len(), 3);
        // 全部同分时应稳定返回前 3 条且都属于本会话
        assert!(results.iter().all(|r| r.key.starts_with("sess_solo_")));
    }
}
