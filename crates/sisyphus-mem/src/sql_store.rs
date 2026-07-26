//! Structured SQL Store for Sisyphus - SQLite/PostgreSQL with proper relational schema.
//! Replaces MfsStore's simple KV with structured tables for SOPs, Traces, Experiences, Vector embeddings.

use anyhow::{Context, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sisyphus_types::{StateStore, TypesError};
use sqlx::{Pool, Row, Sqlite, sqlite::SqlitePoolOptions};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// Write operations for async buffering
#[derive(Debug)]
enum WriteOp {
    Set { key: String, value: Value },
    Delete { key: String },
    Flush(tokio::sync::oneshot::Sender<Result<()>>),
}

/// L1 Cache entry
#[derive(Clone)]
struct CacheEntry {
    value: Value,
    last_access: Instant,
}

/// Structured SQL Store with L1 cache + SQLite/PostgreSQL backend
pub struct SqlStore {
    /// L1 in-memory cache (LRU-like with DashMap)
    l1: Arc<DashMap<String, CacheEntry>>,
    /// Database pool (SQLite or PostgreSQL)
    pool: Pool<Sqlite>,
    /// Async write buffer channel
    write_tx: mpsc::UnboundedSender<WriteOp>,
    /// Configuration
    config: SqlStoreConfig,
}

/// Configuration for SqlStore
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SqlStoreConfig {
    /// L1 cache capacity (number of entries)
    pub l1_capacity: usize,
    /// Enable WAL mode for SQLite
    pub wal_mode: bool,
    /// Synchronous mode for SQLite
    pub synchronous: SqliteSynchronous,
    /// Database URL
    pub database_url: String,
    /// Enable vector similarity search (requires pgvector for Postgres)
    pub enable_vectors: bool,
    /// Vector dimension for embeddings
    pub vector_dimension: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SqliteSynchronous {
    Off,
    Normal,
    Full,
    Extra,
}

impl Default for SqlStoreConfig {
    fn default() -> Self {
        Self {
            l1_capacity: 10_000,
            wal_mode: true,
            synchronous: SqliteSynchronous::Normal,
            database_url: "sqlite:sisyphus.db".to_string(),
            enable_vectors: false,
            vector_dimension: 1536, // OpenAI text-embedding-3-small
        }
    }
}

impl SqlStore {
    /// Creates a new SqlStore with default configuration
    pub async fn new(database_url: &str) -> Result<Self> {
        let config = SqlStoreConfig {
            database_url: database_url.to_string(),
            ..Default::default()
        };
        Self::with_config(config).await
    }

    /// Creates a new SqlStore with custom configuration
    pub async fn with_config(config: SqlStoreConfig) -> Result<Self> {
        // Connect to database
        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .connect(&config.database_url)
            .await
            .context("Failed to connect to database")?;

        // Configure SQLite pragmas
        if config.wal_mode {
            sqlx::query("PRAGMA journal_mode=WAL;")
                .execute(&pool)
                .await?;
        }
        sqlx::query(&format!(
            "PRAGMA synchronous={};",
            match config.synchronous {
                SqliteSynchronous::Off => "OFF",
                SqliteSynchronous::Normal => "NORMAL",
                SqliteSynchronous::Full => "FULL",
                SqliteSynchronous::Extra => "EXTRA",
            }
        ))
        .execute(&pool)
        .await?;

        // Initialize schema
        Self::init_schema(&pool, &config).await?;

        // Setup async write buffer
        let (tx, rx) = mpsc::unbounded_channel::<WriteOp>();
        let pool_clone = pool.clone();
        let config_clone = config.clone();

        tokio::spawn(async move {
            Self::flush_worker(pool_clone, rx, config_clone).await;
        });

        Ok(Self {
            l1: Arc::new(DashMap::new()),
            pool,
            write_tx: tx,
            config,
        })
    }

    /// Initialize database schema
    async fn init_schema(pool: &Pool<Sqlite>, _config: &SqlStoreConfig) -> Result<()> {
        let schema = r#"
        -- Core state table (project-scoped KV)
        CREATE TABLE IF NOT EXISTS state (
            project_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            PRIMARY KEY (project_id, key)
        );

        -- Index for prefix queries
        CREATE INDEX IF NOT EXISTS idx_state_project_prefix ON state(project_id, key);

        -- SOP execution traces
        CREATE TABLE IF NOT EXISTS execution_traces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT NOT NULL,
            sop_id TEXT NOT NULL,
            node_id TEXT NOT NULL,
            status TEXT NOT NULL,
            output TEXT,
            fuel_consumed INTEGER,
            duration_ms INTEGER,
            timestamp INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_traces_sop ON execution_traces(project_id, sop_id);
        CREATE INDEX IF NOT EXISTS idx_traces_node ON execution_traces(project_id, node_id);
        CREATE INDEX IF NOT EXISTS idx_traces_status ON execution_traces(status);

        -- Experiences for learning
        CREATE TABLE IF NOT EXISTS experiences (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            context_text TEXT NOT NULL,
            context_hash TEXT NOT NULL,
            action_id TEXT,
            outcome_success BOOLEAN NOT NULL,
            deviation_score REAL NOT NULL,
            cost INTEGER NOT NULL,
            timestamp INTEGER NOT NULL,
            embedding BLOB,
            visual_embedding BLOB,
            insight TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_exp_project ON experiences(project_id);
        CREATE INDEX IF NOT EXISTS idx_exp_action ON experiences(action_id);
        CREATE INDEX IF NOT EXISTS idx_exp_success ON experiences(outcome_success);
        CREATE INDEX IF NOT EXISTS idx_exp_hash ON experiences(context_hash);

        -- SOP definitions (structured)
        CREATE TABLE IF NOT EXISTS sops (
            id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            name TEXT NOT NULL,
            version TEXT,
            config_json TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            PRIMARY KEY (id, project_id)
        );

        CREATE INDEX IF NOT EXISTS idx_sops_project ON sops(project_id);

        -- SOP nodes
        CREATE TABLE IF NOT EXISTS sop_nodes (
            node_id TEXT PRIMARY KEY,
            sop_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            node_index INTEGER NOT NULL,
            name TEXT NOT NULL,
            kind_type TEXT NOT NULL,
            config_json TEXT NOT NULL,
            FOREIGN KEY(sop_id, project_id) REFERENCES sops(id, project_id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_nodes_sop ON sop_nodes(sop_id, project_id);

        -- SOP edges
        CREATE TABLE IF NOT EXISTS sop_edges (
            sop_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            source_idx INTEGER NOT NULL,
            target_idx INTEGER NOT NULL,
            PRIMARY KEY (sop_id, project_id, source_idx, target_idx),
            FOREIGN KEY(sop_id, project_id) REFERENCES sops(id, project_id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_edges_sop ON sop_edges(sop_id, project_id);

        -- Roadmap structures
        CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            context TEXT,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );

        CREATE TABLE IF NOT EXISTS milestones (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            order_index INTEGER NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            completed_at INTEGER,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_milestones_project ON milestones(project_id);

        CREATE TABLE IF NOT EXISTS phases (
            id TEXT PRIMARY KEY,
            milestone_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            order_index INTEGER NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            completed_at INTEGER,
            FOREIGN KEY(milestone_id) REFERENCES milestones(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_phases_milestone ON phases(milestone_id);

        CREATE TABLE IF NOT EXISTS goals (
            id TEXT PRIMARY KEY,
            phase_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT,
            sop_id TEXT,
            project_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            priority INTEGER NOT NULL DEFAULT 0,
            order_index INTEGER NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            started_at INTEGER,
            completed_at INTEGER,
            FOREIGN KEY(phase_id) REFERENCES phases(id) ON DELETE CASCADE,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_goals_phase ON goals(phase_id);
        CREATE INDEX IF NOT EXISTS idx_goals_project ON goals(project_id);
        CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);

        -- Skill registry
        CREATE TABLE IF NOT EXISTS skills (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            version TEXT NOT NULL,
            author TEXT,
            source_url TEXT,
            config_json TEXT NOT NULL,
            tags TEXT, -- JSON array
            installed_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            is_remote BOOLEAN NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_skills_tags ON skills(tags);

        -- User preferences (project-scoped)
        CREATE TABLE IF NOT EXISTS user_preferences (
            project_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            PRIMARY KEY (project_id, key)
        );

        -- Vector embeddings (for semantic search)
        CREATE TABLE IF NOT EXISTS vectors (
            id TEXT PRIMARY KEY,
            project_id TEXT,
            collection TEXT NOT NULL, -- 'experiences', 'skills', 'documents', etc.
            embedding BLOB NOT NULL,   -- Binary f32 array
            metadata TEXT NOT NULL,    -- JSON metadata
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );

        CREATE INDEX IF NOT EXISTS idx_vectors_project ON vectors(project_id);
        CREATE INDEX IF NOT EXISTS idx_vectors_collection ON vectors(collection);
        "#;

        sqlx::query(schema).execute(pool).await?;
        Ok(())
    }

    /// Async flush worker for batched writes
    async fn flush_worker(
        pool: Pool<Sqlite>,
        mut rx: mpsc::UnboundedReceiver<WriteOp>,
        _config: SqlStoreConfig,
    ) {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        let mut buffer = Vec::new();

        loop {
            tokio::select! {
                Some(op) = rx.recv() => {
                    match op {
                        WriteOp::Set { key, value } => {
                            buffer.push(WriteOp::Set { key, value });
                            if buffer.len() >= 100 {
                                if let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                                    tracing::error!(error = %e, "L2 batch flush failed");
                                }
                            }
                        }
                        WriteOp::Delete { key } => {
                            buffer.push(WriteOp::Delete { key });
                            if buffer.len() >= 100 {
                                if let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                                    tracing::error!(error = %e, "L2 batch flush failed");
                                }
                            }
                        }
                        WriteOp::Flush(tx) => {
                            let res = Self::flush_batch(&pool, &mut buffer).await;
                            let _ = tx.send(res);
                        }
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        if let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                            tracing::error!(error = %e, "L2 periodic flush failed");
                        }
                    }
                }
            }
        }
    }

    async fn flush_batch(pool: &Pool<Sqlite>, buffer: &mut Vec<WriteOp>) -> Result<()> {
        if buffer.is_empty() {
            return Ok(());
        }

        let mut tx = pool.begin().await.context("Failed to begin transaction")?;

        for op in buffer.drain(..) {
            match op {
                WriteOp::Set { key, value } => {
                    // Key format: "project_id:key"
                    let parts: Vec<&str> = key.splitn(2, ':').collect();
                    let (project_id, actual_key) = if parts.len() == 2 {
                        (parts[0], parts[1])
                    } else {
                        ("default", key.as_str())
                    };
                    let value_str = serde_json::to_string(&value)?;
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs() as i64;

                    sqlx::query(
                        r#"INSERT INTO state (project_id, key, value, updated_at) VALUES (?, ?, ?, ?)
                           ON CONFLICT(project_id, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"#
                    )
                    .bind(project_id)
                    .bind(actual_key)
                    .bind(&value_str)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;
                }
                WriteOp::Delete { key } => {
                    let parts: Vec<&str> = key.splitn(2, ':').collect();
                    let (project_id, actual_key) = if parts.len() == 2 {
                        (parts[0], parts[1])
                    } else {
                        ("default", key.as_str())
                    };
                    sqlx::query("DELETE FROM state WHERE project_id = ? AND key = ?")
                        .bind(project_id)
                        .bind(actual_key)
                        .execute(&mut *tx)
                        .await?;
                }
                WriteOp::Flush(_) => unreachable!("Flush op should not be in batch"),
            }
        }

        tx.commit()
            .await
            .context("Failed to commit batch transaction")?;
        Ok(())
    }

    /// Forces all buffered writes to be persisted
    pub async fn flush(&self) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.write_tx
            .send(WriteOp::Flush(tx))
            .map_err(|e| anyhow::anyhow!("Failed to send flush signal: {e}"))?;
        rx.await.context("Flush worker dropped oneshot receiver")?
    }

    /// Get the underlying database pool for custom queries
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    /// Store a vector embedding
    pub async fn store_embedding(
        &self,
        project_id: Option<&str>,
        collection: &str,
        id: &str,
        embedding: &[f32],
        metadata: &Value,
    ) -> Result<()> {
        if !self.config.enable_vectors {
            return Ok(());
        }

        let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        let metadata_str = serde_json::to_string(metadata)?;
        let pid = project_id.unwrap_or("default");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        sqlx::query(
            r#"INSERT INTO vectors (id, project_id, collection, embedding, metadata, created_at)
               VALUES (?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET embedding = excluded.embedding, metadata = excluded.metadata"#
        )
        .bind(id)
        .bind(pid)
        .bind(collection)
        .bind(embedding_bytes)
        .bind(metadata_str)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Search similar vectors using cosine similarity (SQLite fallback - full scan)
    pub async fn search_similar(
        &self,
        project_id: Option<&str>,
        collection: &str,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<(String, f32, Value)>> {
        if !self.config.enable_vectors {
            return Ok(Vec::new());
        }

        let pid = project_id.unwrap_or("default");
        let rows = sqlx::query(
            "SELECT id, embedding, metadata FROM vectors WHERE project_id = ? AND collection = ?",
        )
        .bind(pid)
        .bind(collection)
        .fetch_all(&self.pool)
        .await?;

        let mut scored = Vec::new();
        for row in rows {
            let id: String = row.get("id");
            let embedding_bytes: Vec<u8> = row.get("embedding");
            let metadata_str: String = row.get("metadata");

            // Compute cosine similarity
            let embedding: Vec<f32> = embedding_bytes
                .chunks_exact(4)
                .filter_map(|chunk| {
                    let bytes: [u8; 4] = chunk.try_into().ok()?;
                    Some(f32::from_le_bytes(bytes))
                })
                .collect();

            if embedding.len() != query_embedding.len() {
                continue;
            }

            let score = cosine_similarity(&embedding, query_embedding);
            let metadata: Value = serde_json::from_str(&metadata_str).unwrap_or(Value::Null);
            scored.push((id, score, metadata));
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);

        Ok(scored)
    }

    /// Get L1 cache stats
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            size: self.l1.len(),
            capacity: self.config.l1_capacity,
        }
    }

    /// Clear L1 cache
    pub fn clear_cache(&self) {
        self.l1.clear();
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub size: usize,
    pub capacity: usize,
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

#[async_trait]
impl StateStore for SqlStore {
    async fn get_state_json(
        &self,
        project_id: &str,
        key: &str,
    ) -> Result<Option<Value>, TypesError> {
        // Check L1 cache first
        let cache_key = format!("{project_id}:{key}");
        if let Some(entry) = self.l1.get(&cache_key) {
            return Ok(Some(entry.value.clone()));
        }

        // Check L2 (database)
        let row = sqlx::query("SELECT value FROM state WHERE project_id = ? AND key = ?")
            .bind(project_id)
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| TypesError::StateStoreError(e.to_string()))?;

        if let Some(row) = row {
            let value_str: String = row.get("value");
            let value: Value = serde_json::from_str(&value_str)
                .map_err(|e| TypesError::StateStoreError(e.to_string()))?;

            // Update L1 cache
            self.l1.insert(
                cache_key,
                CacheEntry {
                    value: value.clone(),
                    last_access: Instant::now(),
                },
            );

            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    async fn set_state_json(
        &self,
        project_id: &str,
        key: &str,
        value: Value,
    ) -> Result<(), TypesError> {
        // Update L1 cache
        let cache_key = format!("{project_id}:{key}");
        self.l1.insert(
            cache_key,
            CacheEntry {
                value: value.clone(),
                last_access: Instant::now(),
            },
        );

        // Send to write buffer
        let full_key = format!("{project_id}:{key}");
        self.write_tx
            .send(WriteOp::Set {
                key: full_key,
                value,
            })
            .map_err(|e| TypesError::StateStoreError(e.to_string()))?;

        Ok(())
    }

    async fn get(&self, project_id: &str, key: &str) -> Result<Option<Vec<u8>>, TypesError> {
        let cache_key = format!("{project_id}:{key}");
        if let Some(entry) = self.l1.get(&cache_key) {
            let bytes = serde_json::to_vec(&entry.value)
                .map_err(|e| TypesError::StateStoreError(e.to_string()))?;
            return Ok(Some(bytes));
        }

        let row = sqlx::query("SELECT value FROM state WHERE project_id = ? AND key = ?")
            .bind(project_id)
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| TypesError::StateStoreError(e.to_string()))?;

        if let Some(row) = row {
            let value_str: String = row.get("value");
            let value: Value = serde_json::from_str(&value_str)
                .map_err(|e| TypesError::StateStoreError(e.to_string()))?;
            let bytes = serde_json::to_vec(&value)
                .map_err(|e| TypesError::StateStoreError(e.to_string()))?;

            self.l1.insert(
                cache_key,
                CacheEntry {
                    value,
                    last_access: Instant::now(),
                },
            );

            Ok(Some(bytes))
        } else {
            Ok(None)
        }
    }

    async fn set(&self, project_id: &str, key: &str, value: &[u8]) -> Result<(), TypesError> {
        let json_value: Value = serde_json::from_slice(value)
            .map_err(|e| TypesError::StateStoreError(e.to_string()))?;

        let cache_key = format!("{project_id}:{key}");
        self.l1.insert(
            cache_key.clone(),
            CacheEntry {
                value: json_value.clone(),
                last_access: Instant::now(),
            },
        );

        let full_key = format!("{project_id}:{key}");
        self.write_tx
            .send(WriteOp::Set {
                key: full_key,
                value: json_value,
            })
            .map_err(|e| TypesError::StateStoreError(e.to_string()))?;

        Ok(())
    }

    async fn get_traces_for_sop(
        &self,
        project_id: &str,
        sop_id: &str,
    ) -> Result<Vec<(String, String, String, i64, i64)>, TypesError> {
        let rows = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT node_id, status, output, fuel_consumed, duration_ms 
             FROM execution_traces 
             WHERE project_id = ? AND sop_id = ? 
             ORDER BY timestamp",
        )
        .bind(project_id)
        .bind(sop_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| TypesError::StateStoreError(e.to_string()))?;

        Ok(rows)
    }
}

// Migration from MfsStore
impl SqlStore {
    /// Migrate data from MfsStore to SqlStore
    pub async fn migrate_from_mfs(&self, mfs: &crate::store::MfsStore) -> Result<usize> {
        let mut count = 0;

        // Migrate state keys
        let keys = mfs.list_keys_with_prefix(None, "").await?;
        for key in keys {
            if let Some(value) = mfs.get_state_json("default", &key).await? {
                self.set_state_json("default", &key, value).await?;
                count += 1;
            }
        }

        self.flush().await?;
        Ok(count)
    }
}
