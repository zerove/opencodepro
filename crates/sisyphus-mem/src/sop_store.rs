//! Structured storage for Sisyphus SOPs.
//! Handles the relational mapping of SOPs, their nodes, and their topology.

use anyhow::{Context, Result};
use sisyphus_types::StateStore;
use sisyphus_types::graph::{NodeKind, SOPGraphConfig, SOPNodeConfig};
use sqlx::SqlitePool;

#[derive(Clone)]
pub struct SopSqlStore {
    pool: SqlitePool,
}

impl SopSqlStore {
    pub async fn new(pool: SqlitePool) -> Result<Self> {
        let store = Self { pool };
        store.init_schema().await?;
        Ok(store)
    }

    async fn init_schema(&self) -> Result<()> {
        let schema = r#"
            CREATE TABLE IF NOT EXISTS sops (
                id TEXT,
                project_id TEXT NOT NULL,
                name TEXT NOT NULL,
                version TEXT,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (id, project_id)
            );

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

            CREATE TABLE IF NOT EXISTS sop_edges (
                sop_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                source_idx INTEGER NOT NULL,
                target_idx INTEGER NOT NULL,
                PRIMARY KEY (sop_id, project_id, source_idx, target_idx),
                FOREIGN KEY(sop_id, project_id) REFERENCES sops(id, project_id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_nodes_sop ON sop_nodes(sop_id, project_id);
            CREATE INDEX IF NOT EXISTS idx_edges_sop ON sop_edges(sop_id, project_id);
        "#;

        sqlx::query(schema)
            .execute(&self.pool)
            .await
            .context("Failed to initialize SOP schema")?;
        Ok(())
    }

    /// Saves a full SOP configuration to the database.
    pub async fn save_sop(
        &self,
        sop_id: &str,
        project_id: &str,
        name: &str,
        version: Option<String>,
        config: &SOPGraphConfig,
    ) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin transaction")?;

        // 1. Insert/Update SOP metadata
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        sqlx::query(
            "INSERT INTO sops (id, project_id, name, version, created_at) VALUES (?, ?, ?, ?, ?) 
             ON CONFLICT(id, project_id) DO UPDATE SET name=excluded.name, version=excluded.version"
        )
        .bind(sop_id)
        .bind(project_id)
        .bind(name)
        .bind(version)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        // 2. Clear and rewrite nodes
        sqlx::query("DELETE FROM sop_nodes WHERE sop_id = ? AND project_id = ?")
            .bind(sop_id)
            .bind(project_id)
            .execute(&mut *tx)
            .await?;

        for (idx, node_cfg) in config.nodes.iter().enumerate() {
            let node_id = format!("{}:{}:{}", sop_id, project_id, idx);
            let kind_type = match &node_cfg.kind {
                NodeKind::Wasm { .. } => "Wasm",
                NodeKind::Retry { .. } => "Retry",
                NodeKind::Branch { .. } => "Branch",
                NodeKind::SubGraph { .. } => "SubGraph",
                NodeKind::Test { .. } => "Test",
                _ => "Internal",
            };
            let config_json = serde_json::to_string(&node_cfg.kind)?;

            sqlx::query(
                "INSERT INTO sop_nodes (node_id, sop_id, project_id, node_index, name, kind_type, config_json) 
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(node_id)
            .bind(sop_id)
            .bind(project_id)
            .bind(idx as i64)
            .bind(&node_cfg.name)
            .bind(kind_type)
            .bind(config_json)
            .execute(&mut *tx)
            .await?;
        }

        // 3. Clear and rewrite edges
        sqlx::query("DELETE FROM sop_edges WHERE sop_id = ? AND project_id = ?")
            .bind(sop_id)
            .bind(project_id)
            .execute(&mut *tx)
            .await?;

        for (src, tgt) in &config.edges {
            sqlx::query(
                "INSERT INTO sop_edges (sop_id, project_id, source_idx, target_idx) VALUES (?, ?, ?, ?)"
            )
            .bind(sop_id)
            .bind(project_id)
            .bind(*src as i64)
            .bind(*tgt as i64)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit()
            .await
            .context("Failed to commit SOP transaction")?;
        Ok(())
    }

    /// Loads a full SOP configuration from the database.
    pub async fn load_sop(&self, sop_id: &str, project_id: &str) -> Result<Option<SOPGraphConfig>> {
        let sop_row = sqlx::query_as::<_, (String, String, Option<String>, i64)>(
            "SELECT id, name, version, created_at FROM sops WHERE id = ? AND project_id = ?",
        )
        .bind(sop_id)
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(_) = sop_row else {
            return Ok(None);
        };

        // 1. Load nodes
        let node_rows = sqlx::query_as::<_, (i64, String, String, String)>(
            "SELECT node_index, name, kind_type, config_json FROM sop_nodes WHERE sop_id = ? AND project_id = ? ORDER BY node_index ASC"
        )
        .bind(sop_id)
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;

        let nodes = node_rows
            .into_iter()
            .map(|(_idx, name, _kind_type, config_json)| {
                let kind: NodeKind = serde_json::from_str(&config_json)
                    .expect("Database contained invalid NodeKind JSON");
                SOPNodeConfig { name, kind }
            })
            .collect();

        // 2. Load edges
        let edge_rows = sqlx::query_as::<_, (i64, i64)>(
            "SELECT source_idx, target_idx FROM sop_edges WHERE sop_id = ? AND project_id = ? ORDER BY source_idx ASC"
        )
        .bind(sop_id)
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;

        let edges = edge_rows
            .into_iter()
            .map(|(src, tgt)| (src as usize, tgt as usize))
            .collect();

        Ok(Some(SOPGraphConfig { nodes, edges }))
    }

    pub async fn delete_sop(&self, sop_id: &str, project_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM sops WHERE id = ? AND project_id = ?")
            .bind(sop_id)
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_sops(&self, project_id: Option<&str>) -> Result<Vec<String>> {
        let rows = match project_id {
            Some(pid) => {
                sqlx::query_as::<_, (String,)>("SELECT id FROM sops WHERE project_id = ?")
                    .bind(pid)
                    .fetch_all(&self.pool)
                    .await?
            }
            None => {
                sqlx::query_as::<_, (String,)>("SELECT id FROM sops")
                    .fetch_all(&self.pool)
                    .await?
            }
        };

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Analytical query: Find all SOPs that utilize a specific tool within a project.
    pub async fn find_sops_by_tool(&self, project_id: &str, tool_id: &str) -> Result<Vec<String>> {
        let query_pattern = format!("%\"tool_id\":\"{}\"%", tool_id);
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT sop_id FROM sop_nodes WHERE project_id = ? AND kind_type = 'Wasm' AND config_json LIKE ?"
        )
        .bind(project_id)
        .bind(query_pattern)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }
    /// Migrates SOPs from a legacy MfsStore.
    pub async fn migrate_from_mfs(&self, mfs: &crate::store::MfsStore) -> Result<usize> {
        let keys = mfs.list_keys_with_prefix(None, "sop:").await?;
        let mut count = 0;

        for key in keys {
            if let Some(val) = mfs.get_state_json("default", &key).await? {
                let sop_id = key.strip_prefix("sop:").unwrap_or(&key);
                if let Ok(config) = serde_json::from_value::<SOPGraphConfig>(val) {
                    self.save_sop(sop_id, "default", sop_id, Some("1.0.0".into()), &config)
                        .await?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }
}
