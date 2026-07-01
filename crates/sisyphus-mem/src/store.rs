use anyhow::{Context, Result};
use async_trait::async_trait;
use lru::LruCache;
use sisyphus_metrics::record_mem_operation;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
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
        Self::with_capacity(db_url, NonZeroUsize::new(10_000).unwrap()).await
    }

    /// Creates a new MfsStore with a specific L1 capacity.
    pub async fn with_capacity(db_url: &str, capacity: NonZeroUsize) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(db_url)
            .await
            .context("Failed to connect to SQLite pool")?;

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
        let cache = self
            .l1
            .lock()
            .map_err(|e| anyhow::anyhow!("MFS L1 lock poisoned: {e}"))?;
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
        self.l1
            .lock()
            .map_err(|e| anyhow::anyhow!("MFS L1 lock poisoned: {e}"))?
            .clear();

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
            let mut cache = self.l1.lock().map_err(|e| anyhow::anyhow!("MFS L1 lock poisoned: {e}"))?;
            for (key, value) in keys_values {
                cache.put(key, value);
            }
        }

        let duration = start.elapsed();
        tracing::info!(duration_ms = duration.as_millis(), "Snapshot restored");
        Ok(())
    }

    /// Returns the current L1 capacity.
    pub fn capacity(&self) -> usize {
        self.l1
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cap()
            .get()
    }

    /// Returns the current L1 size.
    pub fn len(&self) -> usize {
        self.l1.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Returns `true` if L1 is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a clone of the underlying SQLite pool.
    pub fn pool(&self) -> SqlitePool {
        self.l2.clone()
    }
}

#[async_trait]
impl sisyphus_types::StateStore for MfsStore {
    async fn get_state_json(&self, key: &str) -> std::result::Result<Option<serde_json::Value>, sisyphus_types::TypesError> {
        let span = tracing::info_span!("mem.get", key = %key);
        let _enter = span.enter();

        let start = Instant::now();
        // 1. Try L1
        {
            let mut cache = self.l1.lock().map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;
            if let Some(val) = cache.get(key) {
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
                .bind(key)
                .fetch_optional(&self.l2)
                .await
                .map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;

        if let Some((_, value_str)) = row {
            let json_val: serde_json::Value =
                serde_json::from_str(&value_str).map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;
            // Update L1
            {
                let mut cache = self.l1.lock().map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;
                cache.put(key.to_string(), json_val.clone());
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

    async fn set_state_json(&self, key: &str, value: serde_json::Value) -> std::result::Result<(), sisyphus_types::TypesError> {
        let span = tracing::info_span!("mem.set", key = %key);
        let _enter = span.enter();

        let start = Instant::now();

        // Update L1 (immediate)
        {
            let mut cache = self.l1.lock().map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;
            cache.put(key.to_string(), value.clone());
        }

        // Queue for L2 persistence (async)
        self.write_tx
            .send(WriteOp::Set {
                key: key.to_string(),
                value,
            })
            .map_err(|e| sisyphus_types::TypesError::StateStoreError(e.to_string()))?;

        let duration = start.elapsed();
        tracing::debug!(duration_ms = duration.as_millis(), "Set queued completed");
        record_mem_operation("set", "L1", true, duration.as_secs_f64());

        Ok(())
    }
}
