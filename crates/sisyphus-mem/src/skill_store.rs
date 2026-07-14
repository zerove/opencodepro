//! Persistence layer for installed Sisyphus skills.
//! Handles the storage, retrieval, and lifecycle of SkillBundles in SQLite.

use sqlx::{SqlitePool, Row};
use serde_json;
use sisyphus_types::skill::{SkillBundle, SkillMetadata};
use sisyphus_types::TypesError;


#[derive(Clone)]
pub struct SkillStore {
    pool: SqlitePool,
}

impl SkillStore {
    /// Create a new SkillStore using the provided SQLite pool.
    pub async fn new(pool: SqlitePool) -> Result<Self, TypesError> {
        let store = Self { pool };
        store.init_schema().await?;
        Ok(store)
    }

    /// Initialize the database schema for installed skills.
    async fn init_schema(&self) -> Result<(), TypesError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS installed_skills (
                id TEXT PRIMARY KEY,
                bundle_json TEXT NOT NULL,
                version TEXT NOT NULL,
                installed_at INTEGER NOT NULL
            )"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| TypesError::StateStoreError(format!("Failed to init skills schema: {e}")))?;
        
        Ok(())
    }

    /// Install a new skill bundle into the store.
    pub async fn install(&self, bundle: SkillBundle) -> Result<(), TypesError> {
        let bundle_json = serde_json::to_string(&bundle)
            .map_err(|e| TypesError::InternalError(format!("Failed to serialize skill bundle: {e}")))?;
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        sqlx::query(
            "INSERT INTO installed_skills (id, bundle_json, version, installed_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                bundle_json = excluded.bundle_json,
                version = excluded.version,
                installed_at = excluded.installed_at"
        )
        .bind(&bundle.metadata.id)
        .bind(bundle_json)
        .bind(&bundle.metadata.version)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| TypesError::StateStoreError(format!("Failed to install skill {}: {e}", bundle.metadata.id)))?;

        Ok(())
    }

    /// Retrieve a skill bundle by its ID.
    pub async fn get(&self, id: &str) -> Result<Option<SkillBundle>, TypesError> {
        let row = sqlx::query("SELECT bundle_json FROM installed_skills WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| TypesError::StateStoreError(format!("Failed to get skill {}: {e}", id)))?;

        if let Some(row) = row {
            let bundle_json: String = row.get(0);
            let bundle: SkillBundle = serde_json::from_str(&bundle_json)
                .map_err(|e| TypesError::InternalError(format!("Failed to deserialize skill bundle: {e}")))?;
            Ok(Some(bundle))
        } else {
            Ok(None)
        }
    }

    /// List all currently installed skills with their metadata.
    pub async fn list(&self) -> Result<Vec<SkillMetadata>, TypesError> {
        let rows = sqlx::query("SELECT bundle_json FROM installed_skills")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| TypesError::StateStoreError(format!("Failed to list skills: {e}")))?;

        let mut skills = Vec::new();
        for row in rows {
            let bundle_json: String = row.get(0);
            if let Ok(bundle) = serde_json::from_str::<SkillBundle>(&bundle_json) {
                skills.push(bundle.metadata);
            }
        }
        Ok(skills)
    }

    /// Remove a skill from the store.
    pub async fn uninstall(&self, id: &str) -> Result<(), TypesError> {
        sqlx::query("DELETE FROM installed_skills WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| TypesError::StateStoreError(format!("Failed to uninstall skill {}: {e}", id)))?;
        
        Ok(())
    }
}
