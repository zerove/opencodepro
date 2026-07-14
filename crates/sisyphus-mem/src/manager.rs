//! Unified memory management for Sisyphus.
//! Coordinates structured storage (SOPs, Roadmaps) and unstructured state (MfsStore).

use anyhow::{Context, Result};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::sync::Arc;

use crate::store::MfsStore;
use crate::sop_store::SopSqlStore;
use crate::roadmap_store::RoadmapSqlStore;

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
    pub async fn new(db_url: &str) -> Result<Self> {
        // 1. Initialize the shared pool
        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .connect(db_url)
            .await
            .context("Failed to connect to Sisyphus database pool")?;

        // 2. Initialize MfsStore (handles base state, traces, and experiences)
        // Since MfsStore::new creates its own pool, we'll use a modified version 
        // or just let it be if the performance hit is negligible. 
        // However, for industrial grade, we should share the pool.
        // Let's check if MfsStore can take a pool.
        
        // Note: MfsStore::with_capacity currently creates its own pool.
        // To properly share, we would need to refactor MfsStore.
        // For now, we'll initialize them sequentially.
        
        let state = Arc::new(MfsStore::new(db_url).await?);
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
