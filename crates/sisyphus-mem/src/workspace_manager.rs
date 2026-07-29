//! Workspace Manager - Multi-workspace isolation and switching.
//! Provides workspace-level isolation for projects, state, and skills.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Workspace configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// Workspace unique identifier
    pub id: String,
    /// Human-readable workspace name
    pub name: String,
    /// Description of what this workspace is for
    pub description: String,
    /// Root directory for this workspace
    pub root_path: PathBuf,
    /// Database URL for this workspace
    pub database_url: String,
    /// Active project ID in this workspace
    pub active_project_id: Option<String>,
    /// Workspace-specific settings
    pub settings: HashMap<String, serde_json::Value>,
}

/// Workspace metadata stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub root_path: String,
    pub active_project_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Multi-workspace manager
pub struct WorkspaceManager {
    /// SQLite pool for workspace metadata
    pool: Pool<Sqlite>,
    /// Active workspace cache
    active: Arc<RwLock<Option<WorkspaceConfig>>>,
    /// All workspace configs cache
    workspaces: Arc<RwLock<HashMap<String, WorkspaceConfig>>>,
}

impl WorkspaceManager {
    /// Create a new workspace manager
    pub async fn new(pool: Pool<Sqlite>) -> Result<Self> {
        Self::init_schema(&pool).await?;

        let manager = Self {
            pool,
            active: Arc::new(RwLock::new(None)),
            workspaces: Arc::new(RwLock::new(HashMap::new())),
        };

        // Load workspaces into cache
        manager.load_workspaces().await?;

        Ok(manager)
    }

    /// Initialize database schema
    async fn init_schema(pool: &Pool<Sqlite>) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT DEFAULT '',
                root_path TEXT NOT NULL,
                active_project_id TEXT,
                settings TEXT DEFAULT '{}',
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            );
            
            CREATE TABLE IF NOT EXISTS workspace_projects (
                workspace_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT DEFAULT '',
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                PRIMARY KEY (workspace_id, project_id),
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Load all workspaces from database into cache
    async fn load_workspaces(&self) -> Result<()> {
        let rows = sqlx::query("SELECT id, name, description, root_path, active_project_id, settings, created_at, updated_at FROM workspaces")
            .fetch_all(&self.pool)
            .await?;

        let mut workspaces = self.workspaces.write().await;

        for row in rows {
            let settings_str: String = row.get("settings");
            let settings: HashMap<String, serde_json::Value> =
                serde_json::from_str(&settings_str).unwrap_or_default();

            let ws = WorkspaceConfig {
                id: row.get("id"),
                name: row.get("name"),
                description: row.get("description"),
                root_path: PathBuf::from(row.get::<String, _>("root_path")),
                database_url: String::new(),
                active_project_id: row.get("active_project_id"),
                settings,
            };

            workspaces.insert(ws.id.clone(), ws);
        }

        Ok(())
    }

    /// Create a new workspace
    pub async fn create_workspace(&self, config: WorkspaceConfig) -> Result<WorkspaceInfo> {
        let now = chrono::Utc::now().timestamp();
        let settings_str = serde_json::to_string(&config.settings)?;

        sqlx::query(
            r#"INSERT INTO workspaces (id, name, description, root_path, active_project_id, settings, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#
        )
        .bind(&config.id)
        .bind(&config.name)
        .bind(&config.description)
        .bind(config.root_path.to_string_lossy().to_string())
        .bind(&config.active_project_id)
        .bind(&settings_str)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Update cache
        self.workspaces
            .write()
            .await
            .insert(config.id.clone(), config.clone());

        Ok(WorkspaceInfo {
            id: config.id,
            name: config.name,
            description: config.description,
            root_path: config.root_path.to_string_lossy().to_string(),
            active_project_id: config.active_project_id,
            created_at: now,
            updated_at: now,
        })
    }

    /// Get workspace by ID
    pub async fn get_workspace(&self, id: &str) -> Option<WorkspaceConfig> {
        self.workspaces.read().await.get(id).cloned()
    }

    /// List all workspaces
    pub async fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.workspaces
            .read()
            .await
            .values()
            .map(|ws| WorkspaceInfo {
                id: ws.id.clone(),
                name: ws.name.clone(),
                description: ws.description.clone(),
                root_path: ws.root_path.to_string_lossy().to_string(),
                active_project_id: ws.active_project_id.clone(),
                created_at: 0,
                updated_at: 0,
            })
            .collect()
    }

    /// Set active workspace
    pub async fn set_active(&self, workspace_id: &str) -> Result<()> {
        if let Some(ws) = self.get_workspace(workspace_id).await {
            *self.active.write().await = Some(ws);
            Ok(())
        } else {
            anyhow::bail!("Workspace not found: {}", workspace_id)
        }
    }

    /// Get active workspace
    pub async fn get_active(&self) -> Option<WorkspaceConfig> {
        self.active.read().await.clone()
    }

    /// Set active project in workspace
    pub async fn set_active_project(&self, workspace_id: &str, project_id: &str) -> Result<()> {
        sqlx::query("UPDATE workspaces SET active_project_id = ?, updated_at = strftime('%s', 'now') WHERE id = ?")
            .bind(project_id)
            .bind(workspace_id)
            .execute(&self.pool)
            .await?;

        // Update cache
        if let Some(ws) = self.workspaces.write().await.get_mut(workspace_id) {
            ws.active_project_id = Some(project_id.to_string());
        }

        Ok(())
    }

    /// Delete workspace
    pub async fn delete_workspace(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM workspaces WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        self.workspaces.write().await.remove(id);

        Ok(())
    }

    /// Transfer a project from one workspace to another.
    pub async fn transfer_project(
        &self,
        project_id: &str,
        from_workspace: &str,
        to_workspace: &str,
    ) -> Result<()> {
        // Verify both workspaces exist
        if self.get_workspace(from_workspace).await.is_none() {
            anyhow::bail!("Source workspace not found: {}", from_workspace);
        }
        if self.get_workspace(to_workspace).await.is_none() {
            anyhow::bail!("Target workspace not found: {}", to_workspace);
        }

        // Update project's workspace assignment
        sqlx::query(
            "UPDATE workspace_projects SET workspace_id = ? WHERE workspace_id = ? AND project_id = ?"
        )
        .bind(to_workspace)
        .bind(from_workspace)
        .bind(project_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Export knowledge (milestones, goals, outcomes) from a workspace's project.
    pub async fn export_knowledge(
        &self,
        project_id: &str,
        workspace_id: &str,
    ) -> Result<serde_json::Value> {
        // Verify workspace exists
        if self.get_workspace(workspace_id).await.is_none() {
            anyhow::bail!("Workspace not found: {}", workspace_id);
        }

        // Collect knowledge from workspace_projects and workspace settings
        let settings = sqlx::query("SELECT settings FROM workspaces WHERE id = ?")
            .bind(workspace_id)
            .fetch_optional(&self.pool)
            .await?
            .and_then(|row| {
                let settings_str: String = row.get("settings");
                serde_json::from_str(&settings_str).ok()
            })
            .unwrap_or(serde_json::Value::Null);

        let projects = sqlx::query_as::<_, (String, String)>(
            "SELECT project_id, description FROM workspace_projects WHERE workspace_id = ? AND project_id = ?"
        )
        .bind(workspace_id)
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;

        let mut project_data = Vec::new();
        for (pid, desc) in projects {
            project_data.push(serde_json::json!({
                "project_id": pid,
                "description": desc,
            }));
        }

        Ok(serde_json::json!({
            "workspace_id": workspace_id,
            "settings": settings,
            "projects": project_data,
            "exported_at": chrono::Utc::now().to_rfc3339(),
        }))
    }

    /// Import knowledge into a workspace.
    pub async fn import_knowledge(
        &self,
        workspace_id: &str,
        knowledge: &serde_json::Value,
    ) -> Result<()> {
        if self.get_workspace(workspace_id).await.is_none() {
            anyhow::bail!("Workspace not found: {}", workspace_id);
        }

        // Import projects from knowledge bundle
        if let Some(projects) = knowledge.get("projects").and_then(|v| v.as_array()) {
            for project in projects {
                if let (Some(pid), Some(desc)) = (
                    project.get("project_id").and_then(|v| v.as_str()),
                    project.get("description").and_then(|v| v.as_str()),
                ) {
                    sqlx::query(
                        r#"INSERT INTO workspace_projects (workspace_id, project_id, name, description)
                           VALUES (?, ?, ?, ?)
                           ON CONFLICT(workspace_id, project_id) DO UPDATE SET description = excluded.description"#
                    )
                    .bind(workspace_id)
                    .bind(pid)
                    .bind(pid)
                    .bind(desc)
                    .execute(&self.pool)
                    .await?;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn test_workspace_crud() -> Result<()> {
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await?;

        let manager = WorkspaceManager::new(pool).await?;

        let config = WorkspaceConfig {
            id: "ws1".to_string(),
            name: "Test Workspace".to_string(),
            description: "A test workspace".to_string(),
            root_path: PathBuf::from("/tmp/test"),
            database_url: "sqlite::memory:".to_string(),
            active_project_id: None,
            settings: HashMap::new(),
        };

        let info = manager.create_workspace(config.clone()).await?;
        assert_eq!(info.id, "ws1");

        let ws = manager.get_workspace("ws1").await;
        assert!(ws.is_some());

        let list = manager.list_workspaces().await;
        assert_eq!(list.len(), 1);

        manager.delete_workspace("ws1").await?;
        assert!(manager.get_workspace("ws1").await.is_none());

        Ok(())
    }
}
