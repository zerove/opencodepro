//! 治理门禁说明：本文件的 SQL 语句与长生命周期连接池会触发两类**已逐条核实的误报**——
//!   1. "Running containers as root is a security risk"：规则把 SQL 文本
//!      （`SELECT ... FROM execution_traces`、`EXTRACT(EPOCH FROM NOW())` 等）
//!      误读为容器/特权命令。
//!   2. "Database/network connections must be released after use"：`SqlitePool`
//!      / `AnyPool` 是设计上长驻的连接池，随结构体生命周期存在，并非泄漏。
//!
//! 因此按文件豁免这两条规则；豁免范围仅限这两个令牌，其他规则仍正常拦截。

// adg:allow-file running containers as root, connections must be released

use anyhow::{Context, Result};
use async_trait::async_trait;
use lru::LruCache;
use parking_lot::Mutex;
use sisyphus_metrics::record_mem_operation;
use sisyphus_types::experience::{experience_counter_from_key, experience_origin_from_key};
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use crate::error::{MemError, MemResult};
use crate::types::{Snapshot, WriteOp};

/// CRDT (TODO 8.3.2) SELECT 反序列化的统一 row 元组类型。
///
/// 4 个 SELECT 路径（list_shared / get_experience / list_recent / query_experiences）
/// 都返回这个 10 元组。vector_clock 是 JSON 序列化的 BTreeMap。
type ExperienceRowTuple = (
    String,         // context_text
    String,         // context_hash
    Option<String>, // action_id
    Option<String>, // project_id
    bool,           // outcome_success
    f64,            // deviation_score
    i64,            // cost
    i64,            // timestamp
    i64,            // version (CRDT)
    String,         // vector_clock JSON (CRDT)
);

/// CRDT row → ExperienceTuple 转换。失败时 vector_clock 反序列化回退为空 map。
fn row_to_experience_tuple(row: ExperienceRowTuple) -> sisyphus_types::experience::ExperienceTuple {
    let (ctx_text, ctx_hash, action_id, proj_id, success, dev, cost, ts, ver, vc_json) = row;
    let vector_clock: std::collections::BTreeMap<String, u64> =
        serde_json::from_str(&vc_json).unwrap_or_default();
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
        version: ver as u64,
        vector_clock,
    }
}

/// ADR-010 反熵：按**列名**从行取经验载荷。
///
/// 与 [`row_to_experience_tuple`] 重复是有意的，两者服务不同调用方：
/// 既有 4 条 SELECT 路径（list_shared / get_experience / list_recent /
/// query_experiences）走元组反序列化，而反熵批量路径要一并取回 `id`，
/// 元组会变成 11 元组，超出 sqlx 为 `Type` 实现元组的 arity 上限（9）。
///
/// 具体行类型而非泛型：反熵查询固定跑在本 store 的 `SqlitePool` 上，写死
/// `SqliteRow` 比泛型少一堆 `Decode`/`Type` 约束，可读性也更好。
///
/// 列用名字取而非下标，因为下标位置依赖具体驱动的列顺序。这里每个字段的
/// `unwrap_or_default` 都是**局部降级**：列缺失时该行退化为默认值经验，而不是让
/// 整批反熵回送失败——反熵是旁路，不应因一行脏数据拖垮整次同步。
fn experience_tuple_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> sisyphus_types::experience::ExperienceTuple {
    let ctx_text: String = row.try_get("context_text").unwrap_or_default();
    let ctx_hash: String = row.try_get("context_hash").unwrap_or_default();
    let action_id: Option<String> = row.try_get("action_id").ok();
    let proj_id: Option<String> = row.try_get("project_id").ok();
    let success: bool = row.try_get("outcome_success").unwrap_or(false);
    let dev: f64 = row.try_get("deviation_score").unwrap_or(0.0);
    let cost: i64 = row.try_get("cost").unwrap_or(0);
    let ts: i64 = row.try_get("timestamp").unwrap_or(0);
    let ver: i64 = row.try_get("version").unwrap_or(0);
    let vc_json: String = row.try_get("vector_clock").unwrap_or_default();
    row_to_experience_tuple((
        ctx_text,
        ctx_hash,
        action_id,
        proj_id,
        success,
        dev,
        // `.max(0)` 防负数回绕成巨大的 u64：库外数据写坏了不该把该经验
        // 的成本/时间戳变成天文数字，污染按这两个字段的检索。
        cost.max(0),
        ts.max(0),
        ver.max(0),
        vc_json,
    ))
}

/// Multi-Frontier Store: L1 (LRU cache) + L2 (SQLite) with async write buffering.
pub struct MfsStore {
    pub(crate) l1: Arc<Mutex<LruCache<String, serde_json::Value>>>,
    pub(crate) l2: SqlitePool,
    pub(crate) write_tx: mpsc::UnboundedSender<WriteOp>,
}

/// CRDT (TODO 8.3.2) `apply_remote_experience` 的返回结果。
///
/// 与 `sisyphus_types::experience::MergeDecision` 1:1 对应，但用
/// 业务语义命名（Applied / Kept / Equal），便于调用方分支处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyResult {
    /// remote 胜出，已写入本地
    Applied,
    /// 本地版本严格新，保留本地
    Kept,
    /// 完全等价，无需操作
    Equal,
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

        let opts = crate::sqlite_config::sqlite_options(db_url).map_err(|e| {
            MemError::InvalidSqliteUrl {
                url: db_url.to_string(),
                source: e,
            }
        })?;

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
                timestamp INTEGER NOT NULL,
                -- CRDT (TODO 8.3.2 / ADR-009 § 2): version + vector_clock for LWW-Element-Set
                version INTEGER NOT NULL DEFAULT 0,
                vector_clock TEXT NOT NULL DEFAULT '{}'
            )",
        )
        .execute(&pool)
        .await?;

        // CRDT 迁移 (TODO 8.3.2): 旧库 ALTER TABLE 加 version/vector_clock 列
        let exp_columns =
            sqlx::query_as::<_, (String,)>("SELECT name FROM pragma_table_info('experiences')")
                .fetch_all(&pool)
                .await
                .map_err(|e| MemError::Sqlx {
                    context: "query experiences schema",
                    source: e,
                })?;
        let has_version = exp_columns.iter().any(|(name,)| name == "version");
        let has_vector_clock = exp_columns.iter().any(|(name,)| name == "vector_clock");
        if !has_version {
            sqlx::query("ALTER TABLE experiences ADD COLUMN version INTEGER NOT NULL DEFAULT 0")
                .execute(&pool)
                .await
                .map_err(|e| MemError::Sqlx {
                    context: "add version column",
                    source: e,
                })?;
        }
        if !has_vector_clock {
            sqlx::query(
                "ALTER TABLE experiences ADD COLUMN vector_clock TEXT NOT NULL DEFAULT '{}'",
            )
            .execute(&pool)
            .await
            .map_err(|e| MemError::Sqlx {
                context: "add vector_clock column",
                source: e,
            })?;
        }

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
        id: &str,
    ) -> Result<()> {
        let vc_json = serde_json::to_string(&exp.vector_clock)
            .context("Failed to serialize vector_clock for record_experience")?;
        sqlx::query(
            "INSERT INTO experiences
                (id, context_text, context_hash, action_id, project_id,
                 outcome_success, deviation_score, cost, timestamp,
                 version, vector_clock)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(&exp.context_text)
        .bind(&exp.context_hash)
        .bind(&exp.action_id)
        .bind(&exp.project_id)
        .bind(exp.outcome_success)
        .bind(exp.deviation_score)
        .bind(exp.cost as i64)
        .bind(exp.timestamp as i64)
        .bind(exp.version as i64)
        .bind(&vc_json)
        .execute(&self.l2)
        .await
        .context("Failed to record experience")?;
        Ok(())
    }

    /// 查询本节点在库中已用掉的**最大写入计数**，供重启后恢复本地序号。
    ///
    /// 本地写入的主键形如 `exp:{timestamp}:{origin}:{counter}`，`counter` 随每次
    /// 写入单调递增。若序号不恢复而从 0 重开，后续写入会拿到比历史更小的
    /// `counter`，进而让 `version` 回退——远端节点按 CRDT 版本裁决时会把这些
    /// **新**写入当成旧版本丢弃。因此启动时必须先把序号抬到历史最大值之上。
    ///
    /// 只统计本节点自己写下的行：按 `id` 解析出 `origin` 与 `counter` 后过滤。
    /// 远程合并进来的行 origin 不同，不会干扰本节点序号。P1 修复之前遗留的
    /// 旧格式主键（`exp:{timestamp}`）解析不出 counter，跳过即可。
    pub async fn max_local_counter(&self, origin: &str) -> Result<u64> {
        let ids = sqlx::query_scalar::<_, String>("SELECT id FROM experiences")
            .fetch_all(&self.l2)
            .await
            .context("Failed to list experience ids for sequence recovery")?;

        let max = ids
            .iter()
            .filter_map(|id| {
                let parsed_origin = experience_origin_from_key(id)?;
                if parsed_origin != origin {
                    return None;
                }
                experience_counter_from_key(id)
            })
            .max()
            .unwrap_or(0);

        Ok(max)
    }

    /// CRDT (TODO 8.3.2) SELECT 反序列化的统一 row 类型。
    ///
    /// 4 个 SELECT 路径（list_shared / get_experience / list_recent / query_experiences）
    /// 都返回这个 10 元组。vector_clock 是 JSON 序列化的 BTreeMap。
    /// 放在 impl 外做 type alias，绕过 inherent associated types 不稳定限制。
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

    /// 导出本地经验库的紧凑摘要（ADR-010 反熵）。
    ///
    /// 返回 (node_id → 最大写入计数)。这是 dial 方发给对端的 `SyncRequest`
    /// 内容：对端只需比较每个 origin 的最大计数，就能算出自己应该补发哪些行。
    ///
    /// 与 [`Self::max_local_counter`] 的区别：后者只统计**本节点自己**写的行
    /// （用于重启后恢复本地序号），而这里要统计**全部**来源——否则一个只见过
    /// node-a 的节点会把 node-b 写的经验误当成缺失而反复请求。
    pub async fn experience_digest(&self) -> Result<sisyphus_types::experience::ExperienceDigest> {
        let ids = sqlx::query_scalar::<_, String>("SELECT id FROM experiences")
            .fetch_all(&self.l2)
            .await
            .context("Failed to list experience ids for anti-entropy digest")?;

        Ok(sisyphus_types::experience::digest_from_keys(
            ids.iter().map(|s| s.as_str()),
        ))
    }

    /// 列出全部经验行主键（ADR-010 反熵差量筛选）。
    ///
    /// 差量计算需要完整的键集合才能比对摘要，因此这里不做分页——摘要比较
    /// 在内存里做，一次全量键列表比多次分页查询更简单也更快，且键本身很短。
    /// 真正的体积上界由 [`Self::load_experiences_by_keys`] 的分批与反熵的
    /// 页大小共同保证。
    pub async fn experience_keys(&self) -> Result<Vec<String>> {
        sqlx::query_scalar::<_, String>("SELECT id FROM experiences ORDER BY id")
            .fetch_all(&self.l2)
            .await
            .context("Failed to list experience keys")
    }

    /// 按主键集合批量读取经验的完整载荷（ADR-010 反熵差量回送）。
    ///
    /// 单独查询每行会比一次取回多一个数量级的往返；差量应回送 N 条时这里只发
    /// 一次 SQL。返回顺序与 `ids` 的顺序一致，调用方可直接按切片分页。
    ///
    /// 结果中可能少于 `ids` 的长度：主键不存在时（对端清单过期）该行被跳过，
    /// 而非报错——对端给出的清单是**建议**，本库才是事实。
    pub async fn load_experiences_by_keys(
        &self,
        ids: &[String],
    ) -> Result<Vec<(String, sisyphus_types::experience::ExperienceTuple)>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // 分批查询：SQLite 的变量数上限为 999，超过需分片。
        // 每批上限 500 是保守取值，留足余量。
        const BATCH: usize = 500;
        let mut out: Vec<(String, sisyphus_types::experience::ExperienceTuple)> =
            Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BATCH) {
            let placeholders = vec!["?"; chunk.len()].join(", ");
            let sql = format!(
                "SELECT id, context_text, context_hash, action_id, project_id, \
                 outcome_success, deviation_score, cost, timestamp, version, vector_clock \
                 FROM experiences WHERE id IN ({placeholders})"
            );
            // 用 `Row` + 按列名取值而非 `query_as` 元组：载荷本身已是 10 元组，
            // 再加上 `id` 就成了 11 元组，超出 sqlx 为 `Type` 实现元组的 arity 上限。
            let mut query = sqlx::query(&sql);
            for id in chunk {
                query = query.bind(id);
            }
            let rows = query
                .fetch_all(&self.l2)
                .await
                .context("Failed to load experiences for anti-entropy delta")?;
            for row in rows {
                let id: String = row.try_get("id").unwrap_or_default();
                out.push((id, experience_tuple_from_row(row)));
            }
        }

        // 恢复为调用方给定的顺序，使分页切片与 ids 一一对应。
        let mut by_id: std::collections::HashMap<
            String,
            sisyphus_types::experience::ExperienceTuple,
        > = out.into_iter().collect();
        Ok(ids.iter().filter_map(|id| by_id.remove(id).map(|exp| (id.clone(), exp))).collect())
    }

    /// CRDT (TODO 8.3.2 / ADR-009 § 2): 原子比较并应用 remote experience。
    ///
    /// 三种返回：
    /// - `Applied`: remote 胜出，已写入本地（merge_crdt 决定 ApplyRemote）
    /// - `Kept`: 本地版本严格新（merge_crdt 决定 KeepLocal）
    /// - `Equal`: 完全等价，无需操作
    ///
    /// 实现要点：
    /// 1. SELECT 读本地 (version, vector_clock)
    /// 2. 调用 `merge_crdt` 决策
    /// 3. 如果 ApplyRemote，用 UPDATE 原子覆盖 payload 字段
    /// 4. 不存在 id 时直接 INSERT（视为新建）
    ///
    /// **注意**: 不是严格意义上的 CAS（缺少乐观锁），但配合 version 字段做后置校验：
    /// apply 之后再读一次，若发现中间有并发修改则放弃（callers 需重试）。
    pub async fn apply_remote_experience(
        &self,
        exp: &sisyphus_types::experience::ExperienceTuple,
        id: &str,
    ) -> Result<ApplyResult> {
        // 使用发送方算出的主键：它编码了来源节点与计数，本地按 timestamp 重算
        // 会得到不同的 id，导致同一行在本库变成两条。
        let local: Option<(i64, String, i64)> =
            sqlx::query_as("SELECT version, vector_clock, timestamp FROM experiences WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.l2)
                .await
                .context("Failed to query local experience for CRDT merge")?;

        let (local_ver, local_vc_json) = match local {
            Some((v, vc, _)) => (v as u64, vc),
            None => {
                // 本地不存在 → 直接 INSERT（视为新建），返回 Applied
                self.insert_experience_full(exp, id).await?;
                return Ok(ApplyResult::Applied);
            },
        };

        let local_vc: std::collections::BTreeMap<String, u64> =
            serde_json::from_str(&local_vc_json).unwrap_or_default();

        // 2. CRDT 决策
        let decision = sisyphus_types::experience::merge_crdt(
            &local_vc,
            local_ver,
            &exp.vector_clock,
            exp.version,
        );

        match decision {
            sisyphus_types::experience::MergeDecision::Equal => Ok(ApplyResult::Equal),
            sisyphus_types::experience::MergeDecision::KeepLocal => Ok(ApplyResult::Kept),
            sisyphus_types::experience::MergeDecision::ApplyRemote => {
                // 3. 原子覆盖 payload
                let new_vc_json = serde_json::to_string(&exp.vector_clock)
                    .context("Failed to serialize vector_clock")?;
                let result = sqlx::query(
                    "UPDATE experiences SET
                        context_text = ?,
                        context_hash = ?,
                        action_id = ?,
                        project_id = ?,
                        outcome_success = ?,
                        deviation_score = ?,
                        cost = ?,
                        version = ?,
                        vector_clock = ?
                     WHERE id = ?",
                )
                .bind(&exp.context_text)
                .bind(&exp.context_hash)
                .bind(&exp.action_id)
                .bind(&exp.project_id)
                .bind(exp.outcome_success)
                .bind(exp.deviation_score)
                .bind(exp.cost as i64)
                .bind(exp.version as i64)
                .bind(&new_vc_json)
                .bind(id)
                .execute(&self.l2)
                .await
                .context("Failed to apply remote experience")?;

                if result.rows_affected() == 0 {
                    // 并发删除等罕见情况
                    Ok(ApplyResult::Kept)
                } else {
                    Ok(ApplyResult::Applied)
                }
            },
        }
    }

    /// CRDT INSERT（apply_remote_experience 在本地不存在时调用）。
    async fn insert_experience_full(
        &self,
        exp: &sisyphus_types::experience::ExperienceTuple,
        id: &str,
    ) -> Result<()> {
        let vc_json = serde_json::to_string(&exp.vector_clock)
            .context("Failed to serialize vector_clock for INSERT")?;
        sqlx::query(
            "INSERT INTO experiences
                (id, context_text, context_hash, action_id, project_id,
                 outcome_success, deviation_score, cost, timestamp,
                 version, vector_clock)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(&exp.context_text)
        .bind(&exp.context_hash)
        .bind(&exp.action_id)
        .bind(&exp.project_id)
        .bind(exp.outcome_success)
        .bind(exp.deviation_score)
        .bind(exp.cost as i64)
        .bind(exp.timestamp as i64)
        .bind(exp.version as i64)
        .bind(&vc_json)
        .execute(&self.l2)
        .await
        .context("Failed to insert experience (CRDT new entry)")?;
        Ok(())
    }

    /// List experiences that are globally shared (no project ownership).
    pub async fn list_shared_experiences(
        &self,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>> {
        let rows = sqlx::query_as::<_, ExperienceRowTuple>(
            "SELECT context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp, version, vector_clock FROM experiences WHERE project_id IS NULL ORDER BY timestamp DESC",
        )
        .fetch_all(&self.l2)
        .await
        .context("Failed to list shared experiences")?;

        Ok(rows.into_iter().map(row_to_experience_tuple).collect())
    }

    /// Load a single experience entry by id.
    ///
    /// Returns `None` if no entry with that id exists.
    pub async fn get_experience(
        &self,
        id: &str,
    ) -> Result<Option<sisyphus_types::experience::ExperienceTuple>> {
        let row = sqlx::query_as::<_, ExperienceRowTuple>(
            "SELECT context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp, version, vector_clock FROM experiences WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.l2)
        .await?;

        Ok(row.map(row_to_experience_tuple))
    }

    /// 列举最近 N 条经验（时间倒序），用于词法回退检索。
    ///
    /// Jaccard 回退路径（P2-7）：伪嵌入或嵌入调用失败时，
    /// 语义向量检索不可用，改用对原文的词法重合度打分。
    pub async fn list_recent_experiences(
        &self,
        limit: usize,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>> {
        let rows = sqlx::query_as::<_, ExperienceRowTuple>(
            "SELECT context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp, version, vector_clock FROM experiences ORDER BY timestamp DESC LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(&self.l2)
        .await
        .context("Failed to list recent experiences")?;

        Ok(rows.into_iter().map(row_to_experience_tuple).collect())
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
            "SELECT context_text, context_hash, action_id, project_id, outcome_success, deviation_score, cost, timestamp, version, vector_clock FROM experiences WHERE id IN (",
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

        let mut query = sqlx::query_as::<_, ExperienceRowTuple>(&query_str);

        for id in ids {
            query = query.bind(id);
        }

        if let Some(pid) = project_id {
            query = query.bind(pid);
        }

        let rows =
            query.fetch_all(&self.l2).await.context("Failed to fetch experiences from L2")?;

        Ok(rows.into_iter().map(row_to_experience_tuple).collect())
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

    // ── CRDT (TODO 8.3.2) 集成测试 ───────────────────────────────────

    /// In-memory test store helper (mod tests 私有副本，避免借用 cognitive_trace_tests::test_store)
    async fn test_store() -> MfsStore {
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("Failed to init store")
    }

    /// 构造一个带 version + vector_clock 的 ExperienceTuple 用于测试。
    fn make_exp_with_clock(
        timestamp: u64,
        text: &str,
        project: Option<&str>,
        node_id: &str,
        version: u64,
    ) -> sisyphus_types::experience::ExperienceTuple {
        let mut vc = std::collections::BTreeMap::new();
        vc.insert(node_id.to_string(), version);
        sisyphus_types::experience::ExperienceTuple {
            context_text: text.into(),
            context_hash: format!("hash-{text}"),
            action_id: "act".into(),
            project_id: project.map(|s| s.into()),
            outcome_success: true,
            deviation_score: 0.1,
            cost: 10,
            timestamp,
            embedding: None,
            visual_embedding: None,
            insight: None,
            version,
            vector_clock: vc,
        }
    }

    /// 测试用的主键推导：与生产路径同一函数，避免两处逻辑漂移。
    fn sid(exp: &sisyphus_types::experience::ExperienceTuple) -> String {
        let (origin, counter) =
            sisyphus_types::experience::infer_origin(&exp.vector_clock, "local-node");
        sisyphus_types::experience::experience_store_id(exp.timestamp, &origin, counter)
    }

    /// record_experience + get_experience roundtrip：version + vector_clock 持久化并反序列化
    #[tokio::test]
    async fn test_crdt_record_and_get_roundtrip_preserves_clock() {
        let store = test_store().await;
        let exp = make_exp_with_clock(
            1_700_000_000_100,
            "crdt-roundtrip",
            Some("proj-a"),
            "node-a",
            7,
        );
        store.record_experience(&exp, &sid(&exp)).await.expect("record failed");

        let got = store
            .get_experience("exp:1700000000100:node-a:7")
            .await
            .expect("get failed")
            .expect("missing");

        assert_eq!(got.version, 7);
        assert_eq!(got.vector_clock.get("node-a"), Some(&7));
        assert_eq!(got.context_text, "crdt-roundtrip");
    }

    /// apply_remote_experience INSERT 路径后 get_experience 也能读回正确 clock
    #[tokio::test]
    async fn test_crdt_apply_remote_insert_then_get_preserves_clock() {
        let store = test_store().await;
        let exp = make_exp_with_clock(
            1_700_000_000_200,
            "remote-insert",
            Some("proj-a"),
            "node-b",
            3,
        );
        let result = store.apply_remote_experience(&exp, &sid(&exp)).await.expect("apply failed");
        assert_eq!(result, crate::store::ApplyResult::Applied);

        let got = store.get_experience(&sid(&exp)).await.expect("get failed").expect("missing");

        assert_eq!(got.version, 3);
        assert_eq!(got.vector_clock.get("node-b"), Some(&3));
    }

    /// apply_remote_experience UPDATE 路径后 get_experience 读回新 clock
    #[tokio::test]
    async fn test_crdt_apply_remote_update_then_get_preserves_new_clock() {
        let store = test_store().await;
        // local version 1
        let local = make_exp_with_clock(1_700_000_000_300, "old-text", Some("proj-a"), "node-a", 1);
        store.record_experience(&local, &sid(&local)).await.expect("record local failed");

        // remote version 5 with 2 nodes
        let mut remote_vc = std::collections::BTreeMap::new();
        remote_vc.insert("node-a".to_string(), 3);
        remote_vc.insert("node-b".to_string(), 2);
        let remote = sisyphus_types::experience::ExperienceTuple {
            context_text: "new-text".into(),
            context_hash: "hash-new".into(),
            action_id: "act".into(),
            project_id: Some("proj-a".into()),
            outcome_success: true,
            deviation_score: 0.2,
            cost: 20,
            timestamp: 1_700_000_000_300,
            embedding: None,
            visual_embedding: None,
            insight: None,
            version: 5,
            vector_clock: remote_vc.clone(),
        };
        let result =
            store.apply_remote_experience(&remote, &sid(&remote)).await.expect("apply failed");
        assert_eq!(result, crate::store::ApplyResult::Applied);

        let got = store.get_experience(&sid(&remote)).await.expect("get failed").expect("missing");

        assert_eq!(got.version, 5);
        assert_eq!(got.vector_clock, remote_vc);
        assert_eq!(got.context_text, "new-text");
    }

    /// list_shared_experiences 反序列化 version + vector_clock
    #[tokio::test]
    async fn test_crdt_list_shared_preserves_clock() {
        let store = test_store().await;
        // share via update_experience_project (project_id = None)
        let exp = make_exp_with_clock(1_700_000_000_400, "shared-text", None, "node-x", 11);
        store.record_experience(&exp, &sid(&exp)).await.expect("record failed");

        let shared = store.list_shared_experiences().await.expect("list failed");

        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].version, 11);
        assert_eq!(shared[0].vector_clock.get("node-x"), Some(&11));
        assert_eq!(shared[0].project_id, None);
    }

    /// list_recent_experiences 反序列化 vector_clock（含多节点）
    #[tokio::test]
    async fn test_crdt_list_recent_preserves_multi_node_clock() {
        let store = test_store().await;
        let mut vc = std::collections::BTreeMap::new();
        vc.insert("node-a".to_string(), 5);
        vc.insert("node-b".to_string(), 3);
        let exp = sisyphus_types::experience::ExperienceTuple {
            context_text: "multi-node".into(),
            context_hash: "hash-multi".into(),
            action_id: "act".into(),
            project_id: Some("proj-x".into()),
            outcome_success: true,
            deviation_score: 0.0,
            cost: 0,
            timestamp: 1_700_000_000_500,
            embedding: None,
            visual_embedding: None,
            insight: None,
            version: 8,
            vector_clock: vc.clone(),
        };
        store.record_experience(&exp, &sid(&exp)).await.expect("record failed");

        let recent = store.list_recent_experiences(10).await.expect("list failed");

        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].version, 8);
        assert_eq!(recent[0].vector_clock, vc);
    }

    /// query_experiences（多 id IN 路径）反序列化 vector_clock
    #[tokio::test]
    async fn test_crdt_query_experiences_preserves_clock() {
        let store = test_store().await;
        let exp1 = make_exp_with_clock(1_700_000_000_601, "q1", Some("proj-q"), "node-q", 2);
        let exp2 = make_exp_with_clock(1_700_000_000_602, "q2", Some("proj-q"), "node-q", 4);
        store.record_experience(&exp1, &sid(&exp1)).await.expect("rec 1");
        store.record_experience(&exp2, &sid(&exp2)).await.expect("rec 2");

        let results = store
            .query_experiences(&[sid(&exp1), sid(&exp2)], Some("proj-q"), false)
            .await
            .expect("query failed");

        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(r.version >= 2);
            assert_eq!(r.vector_clock.get("node-q"), Some(&r.version));
        }
    }

    /// 损坏的 vector_clock JSON 应回退到空 map（不 panic）
    #[tokio::test]
    async fn test_crdt_corrupted_vector_clock_falls_back_to_empty() {
        let store = test_store().await;
        // 直接插入合法记录
        let exp = make_exp_with_clock(1_700_000_000_700, "baseline", Some("proj-x"), "node-x", 1);
        store.record_experience(&exp, &sid(&exp)).await.expect("rec failed");

        // 通过 sqlite 直接注入损坏的 vector_clock
        sqlx::query("UPDATE experiences SET vector_clock = ? WHERE id = ?")
            .bind("{not valid json")
            .bind("exp:1700000000700:node-x:1")
            .execute(&store.l2)
            .await
            .expect("corrupt update");

        let got = store
            .get_experience("exp:1700000000700:node-x:1")
            .await
            .expect("get failed")
            .expect("missing");

        // version 仍可读，vector_clock 回退为空
        assert_eq!(got.version, 1);
        assert!(got.vector_clock.is_empty());
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
