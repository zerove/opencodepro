//! Unified memory management for Sisyphus.
//! Coordinates structured storage (SOPs, Roadmaps) and unstructured state (MfsStore).

use crate::error::MemError;
use anyhow::{Context, Result};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::sync::Arc;

use crate::roadmap_store::RoadmapSqlStore;
use crate::sop_store::SopSqlStore;
use crate::store::MfsStore;

/// The primary entry point for all persistence in Sisyphus.
/// Manages a shared SQLite pool across multiple specialized stores.
#[derive(Clone)]
pub struct SisyphusStore {
    pub state: Arc<MfsStore>,
    pub sop: Arc<SopSqlStore>,
    pub roadmap: Arc<RoadmapSqlStore>,
    pool: SqlitePool,
}

impl SisyphusStore {
    /// Initializes a new SisyphusStore at the given database URL.
    pub async fn new(db_url: &str, config: &sisyphus_types::Config) -> Result<Self> {
        // 1. Initialize the shared pool.
        //    文件型 SQLite 需确保父目录存在（CI 干净环境没有 data/ 目录），
        //    且 create_if_missing 让 db 文件不存在时自动创建。
        // 内存库与文件库走同一套连接选项：内存库同样需要显式
        // `busy_timeout`，否则它会静默回到 sqlx 默认值，使 flush 屏障的
        // 推导失效。
        let opts = crate::sqlite_config::sqlite_options(db_url).map_err(|e| {
            MemError::InvalidSqliteUrl {
                url: db_url.to_string(),
                source: e,
            }
        })?;

        let pool = if db_url == "sqlite::memory:" || db_url == "sqlite::memory" {
            SqlitePoolOptions::new()
                .max_connections(10)
                .connect_with(opts)
                .await
                .context("Failed to connect to in-memory SQLite pool")?
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
            SqlitePoolOptions::new()
                .max_connections(10)
                .connect_with(opts)
                .await
                .context("Failed to connect to Sisyphus database pool")?
        };

        // 2. Initialize MfsStore (handles base state, traces, and experiences)
        let state = Arc::new(MfsStore::new(db_url, &config.cache).await?);
        let sop = Arc::new(SopSqlStore::new(pool.clone()).await?);
        let roadmap = Arc::new(RoadmapSqlStore::new(pool.clone()).await?);

        // 3. Ensure all schemas are initialized
        roadmap.init_schema().await.context("Failed to initialize roadmap schema")?;

        Ok(Self {
            state,
            sop,
            roadmap,
            pool,
        })
    }

    /// Returns the shared SQLite pool for custom queries.
    pub fn pool(&self) -> SqlitePool {
        self.pool.clone()
    }
}
