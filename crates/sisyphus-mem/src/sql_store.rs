//! Structured SQL Store for Sisyphus - SQLite/PostgreSQL with proper relational schema.
//! Replaces MfsStore's simple KV with structured tables for SOPs, Traces, Experiences, Vector embeddings.

use anyhow::{Context, Result};
use crate::error::MemError;
use async_trait::async_trait;
use lru::LruCache;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sisyphus_types::{StateStore, TypesError};
use sqlx::{AnyPool, Row};
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Write operations for async buffering
#[derive(Debug)]
#[expect(dead_code)]
enum WriteOp {
    Set { key: String, value: Value },
    Delete { key: String },
    Flush(tokio::sync::oneshot::Sender<Result<()>>),
}

/// L1 Cache entry
#[derive(Clone)]
struct CacheEntry {
    value: Value,
}

/// Structured SQL Store with L1 cache + SQLite/PostgreSQL backend
pub struct SqlStore {
    /// L1 in-memory cache (LRU)
    l1: Arc<Mutex<LruCache<String, CacheEntry>>>,
    /// Database pool (SQLite or PostgreSQL via AnyPool)
    pool: AnyPool,
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
/// Sqlite Synchronous.
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
        let url = &config.database_url;
        let is_sqlite = url.starts_with("sqlite:");
        let is_postgres = url.starts_with("postgres://") || url.starts_with("postgresql://");

        // Connect to database using AnyPool for SQLite/Postgres compatibility
        let pool: AnyPool = AnyPool::connect(url)
            .await
            .context("Failed to connect to database")?;

        // Backend-specific setup
        if is_sqlite {
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
        } else if is_postgres && config.enable_vectors {
            // Enable pgvector extension
            sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
                .execute(&pool)
                .await?;
        }

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
            l1: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(config.l1_capacity.max(1))
                    .ok_or_else(|| MemError::InvalidL1Capacity(format!("got {}", config.l1_capacity)))?,
            ))),
            pool,
            write_tx: tx,
            config,
        })
    }

    /// Initialize database schema
    async fn init_schema(pool: &AnyPool, config: &SqlStoreConfig) -> Result<()> {
        let is_postgres = config.database_url.starts_with("postgres://")
            || config.database_url.starts_with("postgresql://");

        if is_postgres {
            // PostgreSQL schema (uses vector type for pgvector)
            let schema = r#"
            CREATE TABLE IF NOT EXISTS state (
                project_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                updated_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
                PRIMARY KEY (project_id, key)
            );
            CREATE INDEX IF NOT EXISTS idx_state_project_prefix ON state(project_id, key);

            CREATE TABLE IF NOT EXISTS execution_traces (
                id BIGSERIAL PRIMARY KEY,
                project_id TEXT NOT NULL,
                sop_id TEXT NOT NULL,
                node_id TEXT NOT NULL,
                status TEXT NOT NULL,
                output TEXT,
                fuel_consumed BIGINT,
                duration_ms BIGINT,
                timestamp BIGINT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_traces_sop ON execution_traces(project_id, sop_id);

            CREATE TABLE IF NOT EXISTS experiences (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                context_text TEXT NOT NULL,
                context_hash TEXT NOT NULL,
                action_id TEXT,
                outcome_success BOOLEAN NOT NULL,
                deviation_score DOUBLE PRECISION NOT NULL,
                cost BIGINT NOT NULL,
                timestamp BIGINT NOT NULL,
                insight TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_exp_project ON experiences(project_id);

            CREATE TABLE IF NOT EXISTS sops (
                id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                name TEXT NOT NULL,
                version TEXT,
                config_json TEXT NOT NULL,
                created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
                updated_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
                PRIMARY KEY (id, project_id)
            );
            CREATE INDEX IF NOT EXISTS idx_sops_project ON sops(project_id);

            CREATE TABLE IF NOT EXISTS sop_nodes (
                node_id TEXT PRIMARY KEY,
                sop_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                node_index INTEGER NOT NULL,
                name TEXT NOT NULL,
                kind_type TEXT NOT NULL,
                config_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sop_edges (
                sop_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                source_idx INTEGER NOT NULL,
                target_idx INTEGER NOT NULL,
                PRIMARY KEY (sop_id, project_id, source_idx, target_idx)
            );

            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                context TEXT,
                created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
                updated_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT)
            );
            CREATE TABLE IF NOT EXISTS milestones (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                order_index INTEGER NOT NULL,
                created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
                completed_at BIGINT
            );
            CREATE TABLE IF NOT EXISTS phases (
                id TEXT PRIMARY KEY,
                milestone_id TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                order_index INTEGER NOT NULL,
                created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
                completed_at BIGINT
            );
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
                created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
                started_at BIGINT,
                completed_at BIGINT
            );
            CREATE INDEX IF NOT EXISTS idx_goals_phase ON goals(phase_id);
            CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);

            CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                version TEXT NOT NULL,
                author TEXT,
                source_url TEXT,
                config_json TEXT NOT NULL,
                tags TEXT,
                installed_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
                is_remote BOOLEAN NOT NULL DEFAULT FALSE
            );

            CREATE TABLE IF NOT EXISTS user_preferences (
                project_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                updated_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
                PRIMARY KEY (project_id, key)
            );

            -- Vector embeddings with pgvector
            CREATE TABLE IF NOT EXISTS vectors (
                id TEXT PRIMARY KEY,
                project_id TEXT,
                collection TEXT NOT NULL,
                embedding vector({dim}),
                metadata TEXT NOT NULL,
                created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT)
            );
            "#
            .replace("{dim}", &config.vector_dimension.to_string());

            sqlx::query(&schema).execute(pool).await?;

            // Create ANN index if vectors are enabled
            if config.enable_vectors {
                let idx_sql =
                    "CREATE INDEX IF NOT EXISTS idx_vectors_embedding ON vectors USING hnsw (embedding vector_cosine_ops)"
                        .to_string();
                sqlx::query(&idx_sql).execute(pool).await?;
            }
            Ok(())
        } else {
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
    }

    /// Async flush worker for batched writes
    async fn flush_worker(
        pool: AnyPool,
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
                            if buffer.len() >= 100
                                && let Err(e) = Self::flush_batch(&pool, &mut buffer).await
                            {
                                tracing::error!(error = %e, "L2 batch flush failed");
                            }
                        }
                        WriteOp::Delete { key } => {
                            buffer.push(WriteOp::Delete { key });
                            if buffer.len() >= 100
                                && let Err(e) = Self::flush_batch(&pool, &mut buffer).await
                            {
                                tracing::error!(error = %e, "L2 batch flush failed");
                            }
                        }
                        WriteOp::Flush(tx) => {
                            let res = Self::flush_batch(&pool, &mut buffer).await;
                            let _ = tx.send(res);
                        }
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty()
                        && let Err(e) = Self::flush_batch(&pool, &mut buffer).await
                    {
                        tracing::error!(error = %e, "L2 periodic flush failed");
                    }
                }
            }
        }
    }

    async fn flush_batch(pool: &AnyPool, buffer: &mut Vec<WriteOp>) -> Result<()> {
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
                WriteOp::Flush(_) => {
                    return Err(anyhow::anyhow!(
                        "Flush op should not be in batch (internal invariant violation)"
                    ));
                }
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
            .map_err(|e| MemError::Channel { context: "flush signal", message: format!("{e}") })?;
        rx.await.context("Flush worker dropped oneshot receiver")?
    }

    /// Get the underlying database pool for custom queries
    pub fn pool(&self) -> &AnyPool {
        &self.pool
    }

    /// Store a vector embedding
    ///
    /// - **PostgreSQL/pgvector**: stores as text `[0.1,0.2,...]` (pgvector native text format)
    /// - **SQLite**: stores as raw f32 little-endian bytes for compactness
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

        let is_pgvector = self.config.database_url.starts_with("postgres://")
            || self.config.database_url.starts_with("postgresql://");

        let pid = project_id.unwrap_or("default");
        let metadata_str = serde_json::to_string(metadata)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        if is_pgvector {
            // pgvector text format: '[0.1,0.2,...]' — directly castable to ::vector
            let vector_text = format!(
                "[{}]",
                embedding
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            );

            // Use PostgreSQL-specific query with vector type
            sqlx::query(
                r#"INSERT INTO vectors (id, project_id, collection, embedding, metadata, created_at)
                   VALUES ($1, $2, $3, $4::vector, $5, $6)
                   ON CONFLICT(id) DO UPDATE SET embedding = excluded.embedding, metadata = excluded.metadata"#,
            )
            .bind(id)
            .bind(pid)
            .bind(collection)
            .bind(&vector_text)
            .bind(&metadata_str)
            .bind(now)
            .execute(&self.pool)
            .await?;
        } else {
            // SQLite: compact raw f32 bytes
            let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

            sqlx::query(
                r#"INSERT INTO vectors (id, project_id, collection, embedding, metadata, created_at)
                   VALUES (?, ?, ?, ?, ?, ?)
                   ON CONFLICT(id) DO UPDATE SET embedding = excluded.embedding, metadata = excluded.metadata"#,
            )
            .bind(id)
            .bind(pid)
            .bind(collection)
            .bind(embedding_bytes)
            .bind(&metadata_str)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Search similar vectors using cosine similarity
    ///
    /// - **PostgreSQL/pgvector**: uses the `<->` (cosine distance) operator with ANN index
    /// - **SQLite fallback**: full table scan with in-memory cosine similarity
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

        // Detect backend and dispatch
        let url = &self.config.database_url;
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            self.search_similar_pgvector(project_id, collection, query_embedding, top_k)
                .await
        } else {
            self.search_similar_sqlite(project_id, collection, query_embedding, top_k)
                .await
        }
    }

    /// pgvector-accelerated ANN search using the `<->` operator.
    ///
    /// Requires the `vector` extension to be installed on the Postgres server.
    /// The caller should ensure `CREATE EXTENSION IF NOT EXISTS vector` has been run
    /// (SqlStore::with_config does this automatically when `enable_vectors` is true).
    async fn search_similar_pgvector(
        &self,
        project_id: Option<&str>,
        collection: &str,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<(String, f32, Value)>> {
        let pid = project_id.unwrap_or("default");

        // Build pgvector literal
        let vector_literal = format!(
            "[{}]",
            query_embedding
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        // Use cosine distance (<->) with pgvector
        // The query casts the embedding BLOB to vector type and computes distance
        let rows = sqlx::query_as::<_, (String, f64, String)>(
            r#"SELECT id,
                      (embedding::vector <-> $1::vector) AS distance,
                      metadata
               FROM vectors
               WHERE project_id = $2 AND collection = $3
                 AND embedding IS NOT NULL
               ORDER BY distance
               LIMIT $4"#,
        )
        .bind(&vector_literal)
        .bind(pid)
        .bind(collection)
        .bind(top_k as i64)
        .fetch_all(&self.pool)
        .await
        .context("pgvector ANN search failed")?;

        let results: Vec<(String, f32, Value)> = rows
            .into_iter()
            .map(|(id, distance, metadata_str)| {
                let similarity = (1.0 - distance) as f32;
                let metadata: Value = serde_json::from_str(&metadata_str).unwrap_or(Value::Null);
                (id, similarity, metadata)
            })
            .collect();

        Ok(results)
    }

    /// SQLite fallback: full table scan with in-memory cosine similarity.
    async fn search_similar_sqlite(
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
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| f32::from_le_bytes(*bytes))
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
            size: self.l1.lock().len(),
            capacity: self.config.l1_capacity,
        }
    }

    /// Clear L1 cache
    pub fn clear_cache(&self) {
        self.l1.lock().clear();
    }

    /// Returns the current L1 capacity.
    pub fn capacity(&self) -> usize {
        self.config.l1_capacity
    }

    /// Returns the current L1 size.
    pub fn len(&self) -> usize {
        self.l1.lock().len()
    }

    /// Returns `true` if L1 is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Captures a point-in-time snapshot of the current state.
    pub async fn take_snapshot(&self) -> Result<crate::types::Snapshot> {
        let span = tracing::info_span!("sql_store.snapshot.take");
        let _enter = span.enter();

        let start = std::time::Instant::now();
        let rows = sqlx::query("SELECT key, value FROM state")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch state for snapshot")?;

        let mut data = std::collections::HashMap::new();
        for row in &rows {
            let key: String = row.get("key");
            let value_str: String = row.get("value");
            if let Ok(value) = serde_json::from_str(&value_str) {
                data.insert(key, value);
            }
        }

        let duration = start.elapsed();
        tracing::info!(
            keys = data.len(),
            duration_ms = duration.as_millis(),
            "SqlStore snapshot taken"
        );
        Ok(crate::types::Snapshot { data })
    }

    /// Restores the state from a given snapshot.
    pub async fn restore_snapshot(&self, snapshot: crate::types::Snapshot) -> Result<()> {
        let span = tracing::info_span!("sql_store.snapshot.restore", keys = snapshot.data.len());
        let _enter = span.enter();

        let start = std::time::Instant::now();
        // 1. Clear L1
        self.l1.lock().clear();

        // 2. Clear L2
        sqlx::query("DELETE FROM state")
            .execute(&self.pool)
            .await
            .context("Failed to clear state table")?;

        // 3. Restore from snapshot
        let keys_values: Vec<(String, serde_json::Value)> = snapshot
            .data
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let mut tx = self.pool.begin().await?;
        for (key, value) in &keys_values {
            let value_str = serde_json::to_string(&value)?;
            sqlx::query("INSERT INTO state (project_id, key, value) VALUES ('default', ?, ?)")
                .bind(key)
                .bind(&value_str)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;

        // Update L1
        {
            let mut cache = self.l1.lock();
            for (key, value) in keys_values {
                cache.put(key, CacheEntry { value });
            }
        }

        let duration = start.elapsed();
        tracing::info!(
            duration_ms = duration.as_millis(),
            "SqlStore snapshot restored"
        );
        Ok(())
    }

    /// Lists all keys in the state store that start with the given prefix.
    pub async fn list_keys_with_prefix(
        &self,
        project_id: Option<&str>,
        prefix: &str,
    ) -> Result<Vec<String>> {
        let prefix_query = match project_id {
            Some(pid) => format!("{}:{}%", pid, prefix),
            None => format!("%{}%", prefix),
        };
        let rows: Vec<(String,)> = sqlx::query_as("SELECT key FROM state WHERE key LIKE ?")
            .bind(prefix_query)
            .fetch_all(&self.pool)
            .await?;

        let results = rows
            .into_iter()
            .map(|(k,)| {
                if let Some(pid) = project_id {
                    k.strip_prefix(&format!("{}:", pid))
                        .unwrap_or(&k)
                        .to_string()
                } else {
                    k
                }
            })
            .collect();

        Ok(results)
    }

    /// Record an execution trace event.
    pub async fn record_trace_event(
        &self,
        sop_id: &str,
        node_id: &str,
        status: &str,
        output: Option<String>,
        fuel_consumed: i64,
        duration_ms: i64,
    ) -> Result<()> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        sqlx::query(
            "INSERT INTO execution_traces (project_id, sop_id, node_id, status, output, fuel_consumed, duration_ms, timestamp) 
             VALUES ('default', ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(sop_id)
        .bind(node_id)
        .bind(status)
        .bind(output)
        .bind(fuel_consumed)
        .bind(duration_ms)
        .bind(timestamp)
        .execute(&self.pool)
        .await
        .context("Failed to record trace event")?;

        Ok(())
    }

    /// Get the most frequently failing nodes.
    pub async fn get_most_failed_nodes(&self, min_failures: i64) -> Result<Vec<(String, i64)>> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT node_id, COUNT(*) as failure_count 
             FROM execution_traces 
             WHERE status = 'error' 
             GROUP BY node_id 
             HAVING failure_count >= ?",
        )
        .bind(min_failures)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch failed nodes")?;

        Ok(rows)
    }

    /// Record an experience tuple.
    pub async fn record_experience(
        &self,
        exp: &sisyphus_types::experience::ExperienceTuple,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO experiences (id, project_id, context_text, context_hash, action_id, outcome_success, deviation_score, cost, timestamp) 
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("exp:{}", exp.timestamp))
        .bind(exp.project_id.as_deref().unwrap_or("default"))
        .bind(&exp.context_text)
        .bind(&exp.context_hash)
        .bind(&exp.action_id)
        .bind(exp.outcome_success)
        .bind(exp.deviation_score)
        .bind(exp.cost as i64)
        .bind(exp.timestamp as i64)
        .execute(&self.pool)
        .await
        .context("Failed to record experience")?;
        Ok(())
    }

    /// Get an experience by id.
    pub async fn get_experience(
        &self,
        id: &str,
    ) -> Result<Option<sisyphus_types::experience::ExperienceTuple>> {
        let row = sqlx::query_as::<_, (
            String, String, Option<String>, Option<String>, bool, f64, i64, i64
        )>(
            "SELECT context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp FROM experiences WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((ctx_text, ctx_hash, action_id, proj_id, success, dev, cost, ts)) = row {
            Ok(Some(sisyphus_types::experience::ExperienceTuple {
                context_text: ctx_text,
                context_hash: ctx_hash,
                action_id: action_id.unwrap_or_else(|| "unknown".to_string()),
                project_id: proj_id,
                outcome_success: success,
                deviation_score: dev,
                cost: cost as u64,
                timestamp: ts as u64,
                embedding: None,
                visual_embedding: None,
                insight: None,
            }))
        } else {
            Ok(None)
        }
    }

    /// Query experiences by ids.
    pub async fn query_experiences(
        &self,
        ids: &[String],
        project_id: Option<&str>,
        success_only: bool,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut query_str = String::from(
            "SELECT context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp FROM experiences WHERE id IN (",
        );
        let placeholders: Vec<String> = (0..ids.len()).map(|_| "?".to_string()).collect();
        query_str.push_str(&placeholders.join(","));
        query_str.push(')');

        if project_id.is_some() {
            query_str.push_str(" AND project_id = ?");
        }
        if success_only {
            query_str.push_str(" AND outcome_success = 1");
        }

        let mut query = sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<String>,
                Option<String>,
                bool,
                f64,
                i64,
                i64,
            ),
        >(&query_str);

        for id in ids {
            query = query.bind(id);
        }

        if let Some(pid) = project_id {
            query = query.bind(pid);
        }

        let rows = query
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch experiences")?;

        let experiences = rows
            .into_iter()
            .map(
                |(ctx_text, ctx_hash, action_id, proj_id, success, dev, cost, ts)| {
                    sisyphus_types::experience::ExperienceTuple {
                        context_text: ctx_text,
                        context_hash: ctx_hash,
                        action_id: action_id.unwrap_or_else(|| "unknown".to_string()),
                        project_id: proj_id,
                        outcome_success: success,
                        deviation_score: dev,
                        cost: cost as u64,
                        timestamp: ts as u64,
                        embedding: None,
                        visual_embedding: None,
                        insight: None,
                    }
                },
            )
            .collect();

        Ok(experiences)
    }
}

#[derive(Debug, Clone)]
/// Cache Stats.
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
        {
            let mut cache = self.l1.lock();
            if let Some(entry) = cache.get(&cache_key) {
                return Ok(Some(entry.value.clone()));
            }
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
            self.l1.lock().put(
                cache_key,
                CacheEntry {
                    value: value.clone(),
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
        self.l1.lock().put(
            cache_key,
            CacheEntry {
                value: value.clone(),
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
        {
            let mut cache = self.l1.lock();
            if let Some(entry) = cache.get(&cache_key) {
                let bytes = serde_json::to_vec(&entry.value)
                    .map_err(|e| TypesError::StateStoreError(e.to_string()))?;
                return Ok(Some(bytes));
            }
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

            self.l1.lock().put(cache_key, CacheEntry { value });

            Ok(Some(bytes))
        } else {
            Ok(None)
        }
    }

    async fn set(&self, project_id: &str, key: &str, value: &[u8]) -> Result<(), TypesError> {
        let json_value: Value = serde_json::from_slice(value)
            .map_err(|e| TypesError::StateStoreError(e.to_string()))?;

        let cache_key = format!("{project_id}:{key}");
        self.l1.lock().put(
            cache_key.clone(),
            CacheEntry {
                value: json_value.clone(),
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
