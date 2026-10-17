pub mod manager;
pub use manager::SisyphusStore;
pub mod error;
pub use error::{MemError, MemResult};
pub mod golden_dataset;
pub mod retrieval;
pub mod sop_store;
pub use sop_store::SopSqlStore;
/// Alternative AnyPool-backed store (PostgreSQL / pgvector).
///
/// Unused by every production path — the bridge builds [`store::MfsStore`] via
/// [`manager::SisyphusStore`] — so it is behind an opt-in feature to keep 1340
/// lines out of the default build.
///
/// Warning: its DDL for `projects`/`milestones`/`phases`/`goals`/`sops` differs
/// from the live schema in `sop_store.rs` and `roadmap_store.rs`. Pointing both
/// at one database means whichever `CREATE TABLE IF NOT EXISTS` runs first
/// wins and the other side fails on missing columns.
#[cfg(feature = "alt-sql-store")]
pub mod sql_store;
#[cfg(feature = "alt-sql-store")]
pub use sql_store::{SqlStore, SqlStoreConfig, SqliteSynchronous};
pub mod store;
pub use store::MfsStore;
pub mod sqlite_config;
pub use sqlite_config::{
    SQLITE_BUSY_TIMEOUT, SQLITE_BUSY_TIMEOUT_SECS, TRACE_FLUSH_BARRIER_TIMEOUT,
    TRACE_FLUSH_BARRIER_TIMEOUT_SECS, sqlite_options,
};
pub mod experience_bank;
pub mod roadmap_store;
pub use roadmap_store::RoadmapSqlStore;
pub mod skill_store;
pub use skill_store::SkillStore;
pub mod skill_market;
pub use skill_market::{SkillMarket, SkillMarketConfig};
pub mod types;
pub use types::{Snapshot, WriteOp};
pub mod user_preferences;
pub mod vector_memory;
pub use vector_memory::{ScoredVector, UserMemoryStore, VectorMemoryStore};

#[cfg(test)]
mod tests;
