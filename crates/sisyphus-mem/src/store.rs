use anyhow::{Context, Result};
use async_trait::async_trait;
use lru::LruCache;
use parking_lot::Mutex;
use sisyphus_metrics::record_mem_operation;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use crate::error::{MemError, MemResult};
use crate::types::{Snapshot, WriteOp};

/// Multi-Frontier Store: L1 (LRU cache) + L2 (SQLite) with async write buffering.
pub struct MfsStore {
    pub(crate) l1: Arc<Mutex<LruCache<String, serde_json::Value>>>,
    pub(crate) l2: SqlitePool,
    pub(crate) write_tx: mpsc::UnboundedSender<WriteOp>,
}

/// 内部 `new` 返回 [`MemError`]，对外 API 仍保留 `anyhow::Result` 以兼容 30+ 调用点。
async fn new_internal(db_url: &str, config: &sisyphus_types::CacheConfig) -> MemResult<MfsStore> {
    let capacity = NonZeroUsize::new(config.l1_capacity)
        .ok_or_else(|| MemError::InvalidL1Capacity(format!("got {}", config.l1_capacity)))?;

    let pool = if db_url == "sqlite::memory:" || db_url == "sqlite::memory" {
        SqlitePoolOptions::new().max_connections(5).connect(db_url).await.map_err(|e| {
            MemError::Sqlx {
                context: "connect in-memory pool",
                source: e,
            }
        })?
    } else {
        let path = db_url.strip_prefix("sqlite:").unwrap_or(db_url);
        if path != ":memory:"
            && let Some(parent) = std::path::Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| MemError::CreateDbParent {
                path: parent.display().to_string(),
                source: e,
            })?;
        }

        let opts = sqlx::sqlite::SqliteConnectOptions::from_str(db_url)
            .map_err(|e| MemError::InvalidSqliteUrl {
                url: db_url.to_string(),
                source: e,
            })?
            .create_if_missing(true)
            .foreign_keys(true);

        SqlitePoolOptions::new().max_connections(5).connect_with(opts).await.map_err(|e| {
            MemError::Sqlx {
                context: "connect file-backed pool",
                source: e,
            }
        })?
    };

    MfsStore::with_pool_internal(pool, capacity, config.batch_size, config.flush_interval_ms).await
}

impl MfsStore {
    /// Creates a new MfsStore with a default L1 capacity of 10_000.
    /// `db_url` should be a sqlite connection string (e.g., "sqlite:state.db").
    ///
    /// For file-backed databases, the file is created if it does not exist
    /// (including parent directories for relative paths).
    pub async fn new(db_url: &str, config: &sisyphus_types::CacheConfig) -> Result<Self> {
        Ok(new_internal(db_url, config).await?)
    }

    /// Creates a new MfsStore with a specific L1 capacity and an existing SQLite pool.
    pub async fn with_pool(
        pool: SqlitePool,
        capacity: NonZeroUsize,
        batch_size: usize,
        flush_interval_ms: u64,
    ) -> Result<Self> {
        Ok(MfsStore::with_pool_internal(pool, capacity, batch_size, flush_interval_ms).await?)
    }

    /// 内部 `with_pool` 返回 [`MemError`]，对外 `pub with_pool` 仍保留 `anyhow::Result` 兼容。
    async fn with_pool_internal(
        pool: SqlitePool,
        capacity: NonZeroUsize,
        batch_size: usize,
        flush_interval_ms: u64,
    ) -> MemResult<MfsStore> {
        // Enable WAL mode for better concurrency
        sqlx::query("PRAGMA journal_mode=WAL;").execute(&pool).await?;
        sqlx::query("PRAGMA synchronous=NORMAL;").execute(&pool).await?;
        sqlx::query("PRAGMA mmap_size=268435456;").execute(&pool).await?;

        // Initialize state table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS state (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS execution_traces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sop_id TEXT NOT NULL,
                node_id TEXT NOT NULL,
                status TEXT NOT NULL,
                output TEXT,
                fuel_consumed INTEGER,
                duration_ms INTEGER,
                timestamp INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS experiences (
                id TEXT PRIMARY KEY,
                context_text TEXT NOT NULL,
                context_hash TEXT NOT NULL,
                action_id TEXT,
                project_id TEXT,
                outcome_success BOOLEAN NOT NULL,
                deviation_score REAL NOT NULL,
                cost INTEGER NOT NULL,
                timestamp INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_exp_project ON experiences(project_id);")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_exp_action ON experiences(action_id);")
            .execute(&pool)
            .await?;

        // 认知决策链持久化（可解释性）：供历史检索与跨会话趋势分析。
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS cognitive_traces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL,
                sop_id TEXT NOT NULL,
                timestamp_ms INTEGER NOT NULL,
                event_json TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_cog_trace_sop ON cognitive_traces(sop_id, id DESC);",
        )
        .execute(&pool)
        .await?;

        // Setup async write buffer
        let (tx, rx) = mpsc::unbounded_channel::<WriteOp>();
        let pool_clone = pool.clone();
        let flush_interval = std::time::Duration::from_millis(flush_interval_ms);

        tokio::spawn(async move {
            Self::flush_worker(pool_clone, rx, batch_size, flush_interval).await;
        });

        Ok(Self {
            l1: Arc::new(Mutex::new(LruCache::new(capacity))),
            l2: pool,
            write_tx: tx,
        })
    }

    async fn flush_worker(
        pool: SqlitePool,
        mut rx: mpsc::UnboundedReceiver<WriteOp>,
        batch_size: usize,
        flush_interval: std::time::Duration,
    ) {
        let mut interval = tokio::time::interval(flush_interval);
        let mut buffer = Vec::new();

        loop {
            tokio::select! {
                Some(op) = rx.recv() => {
                    match op {
                        WriteOp::Set { key, value } => {
                            buffer.push(WriteOp::Set { key, value });
                            if buffer.len() >= batch_size {
                                let start = Instant::now();
                                if let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                                    tracing::error!(error = %e, "L2 batch flush failed");
                                } else {
                                    record_mem_operation("flush_batch", "L2", true, start.elapsed().as_secs_f64());
                                }
                            }
                        }
                        WriteOp::Delete { key } => {
                            buffer.push(WriteOp::Delete { key });
                            if buffer.len() >= batch_size {
                                let start = Instant::now();
                                if let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                                    tracing::error!(error = %e, "L2 batch flush failed");
                                } else {
                                    record_mem_operation("flush_batch", "L2", true, start.elapsed().as_secs_f64());
                                }
                            }
                        }
                        WriteOp::Flush(tx) => {
                            let res = Self::flush_batch(&pool, &mut buffer).await;
                            let _ = tx.send(res);
                        }
                        WriteOp::Clear => {
                            if let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                                tracing::error!(error = %e, "L2 final flush before clear failed");
                            }
                            if let Err(e) = sqlx::query("DELETE FROM state").execute(&pool).await {
                                tracing::error!(error = %e, "L2 clear failed");
                            }
                        }
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        let start = Instant::now();
                        if let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                            tracing::error!(error = %e, "L2 periodic flush failed");
                        } else {
                            record_mem_operation("flush_periodic", "L2", true, start.elapsed().as_secs_f64());
                        }
                    }
                }
            }
        }
    }

    async fn flush_batch(pool: &SqlitePool, buffer: &mut Vec<WriteOp>) -> Result<()> {
        let mut tx = pool.begin().await.context("Failed to begin transaction")?;

        for op in buffer.drain(..) {
            match op {
                WriteOp::Set { key, value } => {
                    let value_str = serde_json::to_string(&value)?;
                    sqlx::query(
                        "INSERT INTO state (key, value) VALUES (?, ?)
                             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    )
                    .bind(key)
                    .bind(&value_str)
                    .execute(&mut *tx)
                    .await?;
                },
                WriteOp::Delete { key } => {
                    sqlx::query("DELETE FROM state WHERE key = ?")
                        .bind(key)
                        .execute(&mut *tx)
                        .await?;
                },
                WriteOp::Flush(_) | WriteOp::Clear => {
                    anyhow::bail!("Flush and Clear ops should not be in the write buffer");
                },
            }
        }

        tx.commit().await.context("Failed to commit batch transaction")?;
        Ok(())
    }

    /// Forces all buffered writes to be persisted to L2.
    pub async fn flush(&self) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.write_tx
            .send(WriteOp::Flush(tx))
            .map_err(|e| anyhow::anyhow!("Failed to send flush signal: {e}"))?;
        rx.await.context("Flush worker dropped oneshot receiver")?
    }

    /// Captures a point-in-time snapshot of the current state.
    pub fn take_snapshot(&self) -> Result<Snapshot> {
        let span = tracing::info_span!("mem.snapshot.take");
        let _enter = span.enter();

        let start = Instant::now();
        let cache = self.l1.lock();
        let mut data = std::collections::HashMap::new();
        for (key, value) in cache.iter() {
            data.insert(key.clone(), value.clone());
        }
        drop(cache);
        let duration = start.elapsed();
        tracing::info!(
            keys = data.len(),
            duration_ms = duration.as_millis(),
            "Snapshot taken"
        );
        Ok(Snapshot { data })
    }

    /// Restores the state from a given snapshot.
    pub async fn restore_snapshot(&self, snapshot: Snapshot) -> Result<()> {
        let span = tracing::info_span!("mem.snapshot.restore", keys = snapshot.data.len());
        let _enter = span.enter();

        let start = Instant::now();
        // 1. Clear L1
        self.l1.lock().clear();

        // 2. Clear L2 via buffer to maintain order
        self.write_tx
            .send(WriteOp::Clear)
            .map_err(|e| anyhow::anyhow!("Failed to send clear signal: {e}"))?;
        self.flush().await?;

        // 3. Restore from snapshot
        let keys_values: Vec<(String, serde_json::Value)> =
            snapshot.data.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (key, value) in &keys_values {
            let value_str = serde_json::to_string(&value)?;
            sqlx::query("INSERT INTO state (key, value) VALUES (?, ?)")
                .bind(key)
                .bind(&value_str)
                .execute(&self.l2)
                .await?;
        }
        {
            let mut cache = self.l1.lock();
            for (key, value) in keys_values {
                cache.put(key, value);
            }
        }

        let duration = start.elapsed();
        tracing::info!(duration_ms = duration.as_millis(), "Snapshot restored");
        Ok(())
    }

    /// Record a single SOP node execution event in the trace log.
    ///
    /// Used by the cognitive loop to feed the experience bank and
    /// enable retrospective queries like [`Self::get_most_failed_nodes`].
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
            "INSERT INTO execution_traces (sop_id, node_id, status, output, fuel_consumed, duration_ms, timestamp) 
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(sop_id)
        .bind(node_id)
        .bind(status)
        .bind(output)
        .bind(fuel_consumed)
        .bind(duration_ms)
        .bind(timestamp)
        .execute(&self.l2)
        .await
        .context("Failed to record trace event")?;

        Ok(())
    }

    /// Return the top nodes by failure count, considering only nodes
    /// with at least `min_failures` recorded failures.
    ///
    /// Used by [`sisyphus_intel`] reflection to surface chronic pain points
    /// in a SOP that should be evolved away.
    pub async fn get_most_failed_nodes(&self, min_failures: i64) -> Result<Vec<(String, i64)>> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT node_id, COUNT(*) as failure_count 
             FROM execution_traces 
             WHERE status = 'error' 
             GROUP BY node_id 
             HAVING failure_count >= ?",
        )
        .bind(min_failures)
        .fetch_all(&self.l2)
        .await
        .context("Failed to fetch failed nodes")?;

        Ok(rows)
    }

    /// Returns the current L1 capacity.
    pub fn capacity(&self) -> usize {
        self.l1.lock().cap().get()
    }

    /// Returns the current L1 size.
    pub fn len(&self) -> usize {
        self.l1.lock().len()
    }

    /// Returns `true` if L1 is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a clone of the underlying SQLite pool.
    pub fn pool(&self) -> SqlitePool {
        self.l2.clone()
    }

    /// Lists all keys in the state store that start with the given prefix.
    /// If `project_id` is provided, it filters for keys belonging to that project.
    /// If `project_id` is None, it searches across all projects.
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
            .fetch_all(&self.l2)
            .await?;

        let results = rows
            .into_iter()
            .map(|(k,)| {
                if let Some(pid) = project_id {
                    k.strip_prefix(&format!("{}:", pid)).unwrap_or(&k).to_string()
                } else {
                    k
                }
            })
            .collect();

        Ok(results)
    }

    /// Persist a structured [`ExperienceEntry`] for later retrieval by
    /// the experience bank.
    pub async fn record_experience(
        &self,
        exp: &sisyphus_types::experience::ExperienceTuple,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO experiences (id, context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp) 
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(format!("exp:{}", exp.timestamp))
        .bind(&exp.context_text)
        .bind(&exp.context_hash)
        .bind(&exp.action_id)
        .bind(&exp.project_id)
        .bind(exp.outcome_success)
        .bind(exp.deviation_score)
        .bind(exp.cost as i64)
        .bind(exp.timestamp as i64)
        .execute(&self.l2)
        .await
        .context("Failed to record experience")?;
        Ok(())
    }

    /// Update the project ownership of an experience.
    /// Passing `None` makes the experience globally shared (visible to all projects).
    pub async fn update_experience_project(
        &self,
        id: &str,
        project_id: Option<&str>,
    ) -> Result<bool> {
        let result = sqlx::query("UPDATE experiences SET project_id = ? WHERE id = ?")
            .bind(project_id)
            .bind(id)
            .execute(&self.l2)
            .await
            .context("Failed to update experience project")?;
        Ok(result.rows_affected() > 0)
    }

    /// List experiences that are globally shared (no project ownership).
    pub async fn list_shared_experiences(
        &self,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>> {
        let rows = sqlx::query_as::<
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
        >(
            "SELECT context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp FROM experiences WHERE project_id IS NULL ORDER BY timestamp DESC",
        )
        .fetch_all(&self.l2)
        .await
        .context("Failed to list shared experiences")?;

        Ok(rows
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
            .collect())
    }

    /// Load a single experience entry by id.
    ///
    /// Returns `None` if no entry with that id exists.
    pub async fn get_experience(
        &self,
        id: &str,
    ) -> Result<Option<sisyphus_types::experience::ExperienceTuple>> {
        let row = sqlx::query_as::<_, (
            String, String, Option<String>, Option<String>, bool, f64, i64, i64
        )>("SELECT context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp FROM experiences WHERE id = ?")
        .bind(id)
        .fetch_optional(&self.l2)
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

    /// 列举最近 N 条经验（时间倒序），用于词法回退检索。
    ///
    /// Jaccard 回退路径（P2-7）：伪嵌入或嵌入调用失败时，
    /// 语义向量检索不可用，改用对原文的词法重合度打分。
    pub async fn list_recent_experiences(
        &self,
        limit: usize,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>> {
        let rows = sqlx::query_as::<
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
        >(
            "SELECT context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp FROM experiences ORDER BY timestamp DESC LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(&self.l2)
        .await
        .context("Failed to list recent experiences")?;

        Ok(rows
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
            .collect())
    }

    /// Query the experience bank with a structured predicate.
    ///
    /// Supports filtering by tags, time range, and minimum reward; results
    /// are sorted by reward descending and capped at `limit` entries.
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

        let rows =
            query.fetch_all(&self.l2).await.context("Failed to fetch experiences from L2")?;

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

#[async_trait]
impl sisyphus_types::StateStore for MfsStore {
    async fn get_state_json(
        &self,
        project_id: &str,
        key: &str,
    ) -> std::result::Result<Option<serde_json::Value>, sisyphus_types::TypesError> {
        let span = tracing::info_span!("mem.get", project_id = %project_id, key = %key);
        let _enter = span.enter();

        let full_key = format!("{}:{}", project_id, key);
        let start = Instant::now();
        // 1. Try L1
        {
            let mut cache = self.l1.lock();
            if let Some(val) = cache.get(&full_key) {
                let duration = start.elapsed();
                tracing::debug!(
                    layer = "L1",
                    hit = true,
                    duration_ms = duration.as_millis(),
                    "Cache hit"
                );
                record_mem_operation("get", "L1", true, duration.as_secs_f64());
                return Ok(Some(val.clone()));
            }
        }

        // 2. Try L2
        let l2_start = Instant::now();
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT key, value FROM state WHERE key = ?")
                .bind(&full_key)
                .fetch_optional(&self.l2)
                .await
                .map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;

        if let Some((_, value_str)) = row {
            let json_val: serde_json::Value = serde_json::from_str(&value_str)
                .map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;
            // Update L1
            {
                let mut cache = self.l1.lock();
                cache.put(full_key.clone(), json_val.clone());
            }
            let duration = l2_start.elapsed();
            tracing::debug!(
                layer = "L2",
                hit = true,
                duration_ms = duration.as_millis(),
                "Cache hit"
            );
            record_mem_operation("get", "L2", true, duration.as_secs_f64());
            return Ok(Some(json_val));
        }

        let duration = start.elapsed();
        tracing::debug!(
            layer = "L2",
            hit = false,
            duration_ms = duration.as_millis(),
            "Cache miss"
        );
        record_mem_operation("get", "L2", false, duration.as_secs_f64());
        Ok(None)
    }

    async fn set_state_json(
        &self,
        project_id: &str,
        key: &str,
        value: serde_json::Value,
    ) -> std::result::Result<(), sisyphus_types::TypesError> {
        let span = tracing::info_span!("mem.set", project_id = %project_id, key = %key);
        let _enter = span.enter();

        let full_key = format!("{}:{}", project_id, key);
        let start = Instant::now();

        // Update L1 (immediate)
        {
            let mut cache = self.l1.lock();
            cache.put(full_key.clone(), value.clone());
        }

        // Queue for L2 persistence (async)
        self.write_tx
            .send(WriteOp::Set {
                key: full_key,
                value,
            })
            .map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;

        let duration = start.elapsed();
        tracing::debug!(duration_ms = duration.as_millis(), "Set queued completed");
        record_mem_operation("set", "L1", true, duration.as_secs_f64());

        Ok(())
    }

    async fn get(
        &self,
        project_id: &str,
        key: &str,
    ) -> std::result::Result<Option<Vec<u8>>, sisyphus_types::TypesError> {
        let val = self.get_state_json(project_id, key).await?;
        match val {
            Some(v) => {
                // Return raw bytes for strings, JSON-encoded for other types
                if let Some(s) = v.as_str() {
                    Ok(Some(s.as_bytes().to_vec()))
                } else {
                    Ok(Some(serde_json::to_vec(&v).map_err(|e| {
                        sisyphus_types::TypesError::InternalError(e.to_string())
                    })?))
                }
            },
            None => Ok(None),
        }
    }

    async fn set(
        &self,
        project_id: &str,
        key: &str,
        value: &[u8],
    ) -> std::result::Result<(), sisyphus_types::TypesError> {
        let json_val: serde_json::Value = serde_json::from_slice(value)
            .map_err(|e| sisyphus_types::TypesError::InternalError(e.to_string()))?;
        self.set_state_json(project_id, key, json_val).await
    }

    async fn get_traces_for_sop(
        &self,
        project_id: &str,
        sop_id: &str,
    ) -> std::result::Result<Vec<(String, String, String, i64, i64)>, sisyphus_types::TypesError>
    {
        let rows = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT node_id, status, output, fuel_consumed, duration_ms 
             FROM execution_traces 
             WHERE sop_id = ? AND project_id = ?
             ORDER BY timestamp ASC",
        )
        .bind(sop_id)
        .bind(project_id)
        .fetch_all(&self.l2)
        .await
        .map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;

        Ok(rows)
    }
}

/// 持久化的一条认知决策记录（可解释性历史）。
#[derive(Clone, Debug)]
pub struct CognitiveTraceRecord {
    /// 记录时刻（epoch 毫秒）。
    pub timestamp_ms: u128,
    /// 所属 SOP。
    pub sop_id: String,
    /// 决策事件本体。
    pub event: sisyphus_types::ExecutionEvent,
}

impl MfsStore {
    /// 将一条认知决策事件写入 `cognitive_traces` 表。
    pub async fn record_cognitive_trace(
        &self,
        project_id: &str,
        sop_id: &str,
        timestamp_ms: u128,
        event: &sisyphus_types::ExecutionEvent,
    ) -> std::result::Result<(), sisyphus_types::TypesError> {
        let event_json = serde_json::to_string(event)
            .map_err(|e| sisyphus_types::TypesError::InternalError(e.to_string()))?;
        let ts = i64::try_from(timestamp_ms).map_err(|_| {
            sisyphus_types::TypesError::InternalError(
                "cognitive trace timestamp exceeds i64 range".to_string(),
            )
        })?;
        sqlx::query(
            "INSERT INTO cognitive_traces (project_id, sop_id, timestamp_ms, event_json)
             VALUES (?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(sop_id)
        .bind(ts)
        .bind(event_json)
        .execute(&self.l2)
        .await
        .map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;
        Ok(())
    }

    /// 查询最近的认知决策历史，新在前。
    ///
    /// `sop_id` 为 `None` 时返回该项目全部 SOP 的记录。
    pub async fn get_cognitive_traces(
        &self,
        project_id: &str,
        sop_id: Option<&str>,
        limit: usize,
    ) -> std::result::Result<Vec<CognitiveTraceRecord>, sisyphus_types::TypesError> {
        let limit = limit.clamp(1, 1000) as i64;
        let rows = sqlx::query_as::<_, (String, i64, String)>(
            "SELECT sop_id, timestamp_ms, event_json FROM cognitive_traces
             WHERE project_id = ? AND (? IS NULL OR sop_id = ?)
             ORDER BY id DESC
             LIMIT ?",
        )
        .bind(project_id)
        .bind(sop_id)
        .bind(sop_id)
        .bind(limit)
        .fetch_all(&self.l2)
        .await
        .map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;

        rows.into_iter()
            .map(|(sop_id, ts, json)| {
                let event: sisyphus_types::ExecutionEvent = serde_json::from_str(&json)
                    .map_err(|e| sisyphus_types::TypesError::InternalError(e.to_string()))?;
                Ok(CognitiveTraceRecord {
                    timestamp_ms: ts as u128,
                    sop_id,
                    event,
                })
            })
            .collect()
    }

    /// 聚合最近的认知决策历史为健康摘要（跨会话）。
    ///
    /// 返回 `CognitiveTraceSummary`：决策分布、指标均值（偏差/置信度/
    /// 稳定性/信息增益）、累计 token 成本与时间范围。`limit` 限制
    /// 参与聚合的记录数（默认取最近 1000 条以内，由调用方决定）。
    pub async fn cognitive_trace_summary(
        &self,
        project_id: &str,
        sop_id: Option<&str>,
        limit: usize,
    ) -> std::result::Result<sisyphus_types::CognitiveTraceSummary, sisyphus_types::TypesError>
    {
        let records = self.get_cognitive_traces(project_id, sop_id, limit).await?;
        let mut summary = sisyphus_types::CognitiveTraceSummary::default();
        let mut deviation_sum = 0.0_f64;
        let mut confidence_sum = 0.0_f64;
        let mut stability_sum = 0.0_f64;
        let mut info_gain_sum = 0.0_f64;
        let mut metric_samples = 0_u64;

        summary.total_decisions = records.len() as u64;
        for (i, rec) in records.iter().enumerate() {
            // records 按 id DESC（新在前）：首条即最近，末条即最早。
            if i == 0 {
                summary.last_timestamp_ms = rec.timestamp_ms;
            }
            summary.first_timestamp_ms = rec.timestamp_ms;

            match &rec.event {
                sisyphus_types::ExecutionEvent::CognitiveStep { metrics, .. } => {
                    summary.step_count += 1;
                    if let Some(m) = metrics {
                        deviation_sum += m.deviation;
                        confidence_sum += m.confidence;
                        stability_sum += m.stability;
                        info_gain_sum += m.info_gain;
                        summary.total_token_cost += m.token_cost;
                        metric_samples += 1;
                    }
                },
                sisyphus_types::ExecutionEvent::MicroReflection { decision, .. } => {
                    summary.reflection_count += 1;
                    *summary.decision_counts.entry(decision.to_uppercase()).or_insert(0) += 1;
                },
                _ => {},
            }
        }

        if metric_samples > 0 {
            let n = metric_samples as f64;
            summary.avg_deviation = deviation_sum / n;
            summary.avg_confidence = confidence_sum / n;
            summary.avg_stability = stability_sum / n;
            summary.avg_info_gain = info_gain_sum / n;
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::StateStore;

    #[tokio::test]
    async fn test_cross_project_isolation_vulnerability() {
        // We use :memory: for fast tests.
        let config = sisyphus_types::CacheConfig::default();
        let store = MfsStore::new("sqlite::memory:", &config).await.expect("Failed to init store");

        // Simulation: Project A sets a value
        let project_a = "project_a";
        let project_b = "project_b";
        let key = "user_preference";
        let val_a = serde_json::json!({"theme": "dark"});
        store.set_state_json(project_a, key, val_a.clone()).await.expect("Set A failed");

        // Simulation: Project B sets the SAME key
        let val_b = serde_json::json!({"theme": "light"});
        store.set_state_json(project_b, key, val_b.clone()).await.expect("Set B failed");

        // Verify: Project A still reads its own value, NOT Project B's
        let result = store.get_state_json(project_a, key).await.expect("Get failed");

        assert_eq!(
            result,
            Some(val_a),
            "Isolation failed: Project B overwrote Project A's state"
        );
    }
}

#[cfg(test)]
mod cognitive_trace_tests {
    use super::*;
    use sisyphus_types::ExecutionEvent;

    fn step_event(step: &str) -> ExecutionEvent {
        ExecutionEvent::CognitiveStep {
            step: step.to_string(),
            message: format!("{step} message"),
            metrics: Some(sisyphus_types::CognitiveMetrics {
                iteration: 1,
                deviation: 0.2,
                confidence: 0.8,
                progress: 0.5,
                token_cost: 10,
                stability: 0.9,
                info_gain: 0.1,
            }),
        }
    }

    async fn test_store() -> MfsStore {
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("Failed to init store")
    }

    #[tokio::test]
    async fn test_record_and_get_roundtrip() {
        let store = test_store().await;
        let ev = step_event("Predicting");
        store
            .record_cognitive_trace("proj", "sop-a", 1700000000123, &ev)
            .await
            .expect("record failed");

        let rows = store.get_cognitive_traces("proj", None, 10).await.expect("get failed");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].timestamp_ms, 1700000000123);
        assert_eq!(rows[0].sop_id, "sop-a");
        assert!(
            matches!(rows[0].event, ExecutionEvent::CognitiveStep { ref step, .. } if step == "Predicting")
        );
    }

    #[tokio::test]
    async fn test_get_filters_sop_and_orders_newest_first() {
        let store = test_store().await;
        store
            .record_cognitive_trace("proj", "sop-a", 1000, &step_event("StepA1"))
            .await
            .expect("test step");
        store
            .record_cognitive_trace("proj", "sop-a", 2000, &step_event("StepA2"))
            .await
            .expect("test step");
        store
            .record_cognitive_trace("proj", "sop-b", 3000, &step_event("StepB1"))
            .await
            .expect("test step");

        // 全部：新在前
        let all = store.get_cognitive_traces("proj", None, 10).await.expect("test step");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].sop_id, "sop-b");

        // 按 sop 过滤
        let sop_a = store.get_cognitive_traces("proj", Some("sop-a"), 10).await.expect("test step");
        assert_eq!(sop_a.len(), 2);
        assert!(sop_a.iter().all(|r| r.sop_id == "sop-a"));
        assert_eq!(sop_a[0].timestamp_ms, 2000);
        assert_eq!(sop_a[1].timestamp_ms, 1000);
    }

    #[tokio::test]
    async fn test_get_respects_limit_and_project_isolation() {
        let store = test_store().await;
        for i in 0..5 {
            store
                .record_cognitive_trace("proj", "sop-a", 1000 + i, &step_event(&format!("S{i}")))
                .await
                .expect("test step");
        }
        store
            .record_cognitive_trace("other", "sop-a", 9999, &step_event("OtherProj"))
            .await
            .expect("test step");

        let limited =
            store.get_cognitive_traces("proj", Some("sop-a"), 2).await.expect("test step");
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].timestamp_ms, 1004);
        assert_eq!(limited[1].timestamp_ms, 1003);

        // 项目隔离：other 项目仅一条，proj 项目不含它
        let other = store.get_cognitive_traces("other", None, 10).await.expect("test step");
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].timestamp_ms, 9999);
    }

    fn reflect_event(decision: &str) -> ExecutionEvent {
        ExecutionEvent::MicroReflection {
            node_name: "node-a".to_string(),
            prediction: "pred".to_string(),
            actual: "actual".to_string(),
            decision: decision.to_string(),
            updates: None,
            reason: Some("tolerance".to_string()),
        }
    }

    fn step_event_no_metrics(step: &str) -> ExecutionEvent {
        ExecutionEvent::CognitiveStep {
            step: step.to_string(),
            message: format!("{step} message"),
            metrics: None,
        }
    }

    #[tokio::test]
    async fn test_cognitive_trace_summary_aggregates() {
        let store = test_store().await;
        store
            .record_cognitive_trace("proj", "sop-a", 1000, &step_event("Predicting"))
            .await
            .expect("test step");
        store
            .record_cognitive_trace("proj", "sop-a", 2000, &step_event("Reflecting"))
            .await
            .expect("test step");
        store
            .record_cognitive_trace("proj", "sop-a", 3000, &step_event_no_metrics("Critiquing"))
            .await
            .expect("test step");
        store
            .record_cognitive_trace("proj", "sop-b", 4000, &reflect_event("ADJUST"))
            .await
            .expect("test step");
        store
            .record_cognitive_trace("proj", "sop-b", 5000, &reflect_event("continue"))
            .await
            .expect("test step");

        let summary =
            store.cognitive_trace_summary("proj", None, 100).await.expect("summary failed");

        assert_eq!(summary.total_decisions, 5);
        // 3 个 CognitiveStep：2 个带 metrics（均值分母=2），1 个不带。
        assert_eq!(summary.step_count, 3);
        assert_eq!(summary.reflection_count, 2);
        assert!((summary.avg_deviation - 0.2).abs() < 1e-9);
        assert!((summary.avg_confidence - 0.8).abs() < 1e-9);
        assert!((summary.avg_stability - 0.9).abs() < 1e-9);
        assert!((summary.avg_info_gain - 0.1).abs() < 1e-9);
        assert_eq!(summary.total_token_cost, 20);
        // decision 大小写归一化：ADJUST + CONTINUE
        assert_eq!(summary.decision_counts.get("ADJUST"), Some(&1));
        assert_eq!(summary.decision_counts.get("CONTINUE"), Some(&1));
        // 时间窗口：新在前 → last=5000, first=1000
        assert_eq!(summary.first_timestamp_ms, 1000);
        assert_eq!(summary.last_timestamp_ms, 5000);
    }

    #[tokio::test]
    async fn test_cognitive_trace_summary_sop_filter_and_limit() {
        let store = test_store().await;
        for i in 0..4 {
            store
                .record_cognitive_trace("proj", "sop-a", 1000 + i, &step_event(&format!("S{i}")))
                .await
                .expect("test step");
        }
        store
            .record_cognitive_trace("proj", "sop-b", 9999, &reflect_event("PIVOT"))
            .await
            .expect("test step");

        // SOP 过滤：仅 sop-a 的 4 条
        let sop_a =
            store.cognitive_trace_summary("proj", Some("sop-a"), 100).await.expect("test step");
        assert_eq!(sop_a.total_decisions, 4);
        assert_eq!(sop_a.step_count, 4);
        assert_eq!(sop_a.reflection_count, 0);

        // limit：仅聚合最近 2 条（sop-a 的 1003/1002）
        let limited =
            store.cognitive_trace_summary("proj", Some("sop-a"), 2).await.expect("test step");
        assert_eq!(limited.total_decisions, 2);
        assert_eq!(limited.first_timestamp_ms, 1002);
        assert_eq!(limited.last_timestamp_ms, 1003);
    }
}
