use anyhow::{Context, Result};
use async_trait::async_trait;
use lru::LruCache;
use parking_lot::Mutex;
use sisyphus_metrics::record_mem_operation;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use crate::types::{Snapshot, WriteOp};

/// Multi-Frontier Store: L1 (LRU cache) + L2 (SQLite) with async write buffering.
pub struct MfsStore {
    pub(crate) l1: Arc<Mutex<LruCache<String, serde_json::Value>>>,
    pub(crate) l2: SqlitePool,
    pub(crate) write_tx: mpsc::UnboundedSender<WriteOp>,
}

impl MfsStore {
    /// Creates a new MfsStore with a default L1 capacity of 10_000.
    /// `db_url` should be a sqlite connection string (e.g., "sqlite:state.db").
    pub async fn new(db_url: &str) -> Result<Self> {
        let capacity = NonZeroUsize::new(10_000).ok_or_else(|| anyhow::anyhow!("Invalid default capacity"))?;
        Self::with_pool(
            SqlitePoolOptions::new()
                .max_connections(5)
                .connect(db_url)
                .await
                .context("Failed to connect to SQLite pool")?,
            capacity,
        ).await
    }


    /// Creates a new MfsStore with a specific L1 capacity and an existing SQLite pool.
    pub async fn with_pool(pool: SqlitePool, capacity: NonZeroUsize) -> Result<Self> {
        // Enable WAL mode for better concurrency
        sqlx::query("PRAGMA journal_mode=WAL;")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA synchronous=NORMAL;")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA mmap_size=268435456;")
            .execute(&pool)
            .await?;

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

        // Setup async write buffer
        let (tx, rx) = mpsc::unbounded_channel::<WriteOp>();
        let pool_clone = pool.clone();

        tokio::spawn(async move {
            Self::flush_worker(pool_clone, rx).await;
        });

        Ok(Self {
            l1: Arc::new(Mutex::new(LruCache::new(capacity))),
            l2: pool,
            write_tx: tx,
        })
    }

    async fn flush_worker(pool: SqlitePool, mut rx: mpsc::UnboundedReceiver<WriteOp>) {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        let mut buffer = Vec::new();

        loop {
            tokio::select! {
                Some(op) = rx.recv() => {
                    match op {
                        WriteOp::Set { key, value } => {
                            buffer.push(WriteOp::Set { key, value });
                            if buffer.len() >= 100
                                && let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                                    tracing::error!(error = %e, "L2 batch flush failed");
                                }
                        }
                        WriteOp::Delete { key } => {
                            buffer.push(WriteOp::Delete { key });
                            if buffer.len() >= 100
                                && let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                                    tracing::error!(error = %e, "L2 batch flush failed");
                                }
                        }
                        WriteOp::Flush(tx) => {
                            let res = Self::flush_batch(&pool, &mut buffer).await;
                            let _ = tx.send(res);
                        }
                        WriteOp::Clear => {
                            // Flush current buffer first
                            if let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                                tracing::error!(error = %e, "L2 final flush before clear failed");
                            }
                            // Now clear L2
                            if let Err(e) = sqlx::query("DELETE FROM state").execute(&pool).await {
                                tracing::error!(error = %e, "L2 clear failed");
                            }
                        }
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty()
                        && let Err(e) = Self::flush_batch(&pool, &mut buffer).await {
                            tracing::error!(error = %e, "L2 periodic flush failed");
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
                }
                WriteOp::Delete { key } => {
                    sqlx::query("DELETE FROM state WHERE key = ?")
                        .bind(key)
                        .execute(&mut *tx)
                        .await?;
                }
                WriteOp::Flush(_) | WriteOp::Clear => {
                    anyhow::bail!("Flush and Clear ops should not be in the write buffer");
                }
            }
        }

        tx.commit()
            .await
            .context("Failed to commit batch transaction")?;
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
        let keys_values: Vec<(String, serde_json::Value)> = snapshot
            .data
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
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

    pub async fn get_most_failed_nodes(&self, min_failures: i64) -> Result<Vec<(String, i64)>> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT node_id, COUNT(*) as failure_count 
             FROM execution_traces 
             WHERE status = 'error' 
             GROUP BY node_id 
             HAVING failure_count >= ?"
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
    pub async fn list_keys_with_prefix(&self, project_id: Option<&str>, prefix: &str) -> Result<Vec<String>> {
        let prefix_query = match project_id {
            Some(pid) => format!("{}:{}%", pid, prefix),
            None => format!("%{}%", prefix),
        };
        let rows: Vec<(String,)> = sqlx::query_as("SELECT key FROM state WHERE key LIKE ?")
            .bind(prefix_query)
            .fetch_all(&self.l2)
            .await?;
        
        let results = rows.into_iter().map(|(k,)| {
            if let Some(pid) = project_id {
                k.strip_prefix(&format!("{}:", pid)).unwrap_or(&k).to_string()
            } else {
                k
            }
        }).collect();

        Ok(results)
    }

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

    pub async fn get_experience(&self, id: &str) -> Result<Option<sisyphus_types::experience::ExperienceTuple>> {
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

    pub async fn query_experiences(
        &self,
        ids: &[String],
        project_id: Option<&str>,
        success_only: bool,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut query_str = String::from("SELECT context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp FROM experiences WHERE id IN (");
        let placeholders: Vec<String> = (0..ids.len()).map(|_| "?".to_string()).collect();
        query_str.push_str(&placeholders.join(","));
        query_str.push_str(")");

        if project_id.is_some() {
            query_str.push_str(" AND project_id = ?");
        }
        if success_only {
            query_str.push_str(" AND outcome_success = 1");
        }

        let mut query = sqlx::query_as::<_, (
            String, String, Option<String>, Option<String>, bool, f64, i64, i64
        )>(&query_str);

        for id in ids {
            query = query.bind(id);
        }

        if let Some(pid) = project_id {
            query = query.bind(pid);
        }

        let rows = query
            .fetch_all(&self.l2)
            .await
            .context("Failed to fetch experiences from L2")?;

        let experiences = rows.into_iter().map(|(ctx_text, ctx_hash, action_id, proj_id, success, dev, cost, ts)| {
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
        }).collect();

        Ok(experiences)
    }
}

#[async_trait]
impl sisyphus_types::StateStore for MfsStore {
    async fn get_state_json(&self, project_id: &str, key: &str) -> std::result::Result<Option<serde_json::Value>, sisyphus_types::TypesError> {
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
            let json_val: serde_json::Value =
                serde_json::from_str(&value_str).map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;
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

    async fn set_state_json(&self, project_id: &str, key: &str, value: serde_json::Value) -> std::result::Result<(), sisyphus_types::TypesError> {
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

    async fn get(&self, project_id: &str, key: &str) -> std::result::Result<Option<Vec<u8>>, sisyphus_types::TypesError> {
        let val = self.get_state_json(project_id, key).await?;
        match val {
            Some(v) => Ok(Some(serde_json::to_vec(&v).map_err(|e| sisyphus_types::TypesError::InternalError(e.to_string()))?)),
            None => Ok(None),
        }
    }

    async fn set(&self, project_id: &str, key: &str, value: &[u8]) -> std::result::Result<(), sisyphus_types::TypesError> {
        let json_val: serde_json::Value = serde_json::from_slice(value)
            .map_err(|e| sisyphus_types::TypesError::InternalError(e.to_string()))?;
        self.set_state_json(project_id, key, json_val).await
    }

    async fn get_traces_for_sop(&self, project_id: &str, sop_id: &str) -> std::result::Result<Vec<(String, String, String, i64, i64)>, sisyphus_types::TypesError> {
        let rows = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT node_id, status, output, fuel_consumed, duration_ms 
             FROM execution_traces 
             WHERE sop_id = ? AND project_id = ?
             ORDER BY timestamp ASC"
        )
        .bind(sop_id)
        .bind(project_id)
        .fetch_all(&self.l2)
        .await
        .map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;
    
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::StateStore;

    #[tokio::test]
    async fn test_cross_project_isolation_vulnerability() {
        // We use :memory: for fast tests.
        let store = MfsStore::new("sqlite::memory:").await.expect("Failed to init store");

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
        
        assert_eq!(result, Some(val_a), "Isolation failed: Project B overwrote Project A's state");
    }
}
