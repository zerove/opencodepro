//! Workspace management for Sisyphus.
//! Handles the lifecycle and switching of independent project environments.

use serde::{Deserialize, Serialize};
use sisyphus_types::{StateStore, TypesError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub project_id: String,
    pub root_dir: String,
    /// Overrides for the global configuration (e.g., different LLM model for this project).
    pub config_overrides: HashMap<String, serde_json::Value>,
}

pub struct WorkspaceManager {
    store: Arc<dyn StateStore>,
    current_workspace: RwLock<Option<String>>,
    workspaces: RwLock<HashMap<String, Workspace>>,
}

impl WorkspaceManager {
    pub fn new(store: Arc<dyn StateStore>) -> Self {
        Self {
            store,
            current_workspace: RwLock::new(None),
            workspaces: RwLock::new(HashMap::new()),
        }
    }

    /// Initialize the manager by loading workspaces from the state store.
    pub async fn init(&self) -> Result<(), TypesError> {
        // Load all workspaces
        if let Some(data) = self.store.get("sys", "sys.workspaces").await? {
            let ws_list: Vec<Workspace> = serde_json::from_slice(&data).map_err(|e| {
                TypesError::InternalError(format!("Failed to deserialize workspaces: {e}"))
            })?;

            let mut ws_map = self.workspaces.write().await;
            for ws in ws_list {
                ws_map.insert(ws.id.clone(), ws);
            }
        }

        // Load current active workspace
        if let Some(id_bytes) = self.store.get("sys", "sys.current_workspace").await? {
            let id = String::from_utf8_lossy(&id_bytes).to_string();
            let mut current = self.current_workspace.write().await;
            *current = Some(id);
        }

        Ok(())
    }

    /// Create a new workspace and persist it.
    pub async fn create_workspace(
        &self,
        name: String,
        project_id: String,
        root_dir: String,
    ) -> Result<Workspace, TypesError> {
        let id = uuid::Uuid::new_v4().to_string();
        let ws = Workspace {
            id: id.clone(),
            name,
            project_id,
            root_dir,
            config_overrides: HashMap::new(),
        };

        // Update memory
        {
            let mut ws_map = self.workspaces.write().await;
            ws_map.insert(id.clone(), ws.clone());
        }

        // Persist to store
        self.persist_workspaces().await?;

        Ok(ws)
    }

    /// Switch the active workspace.
    pub async fn switch_workspace(&self, id: &str) -> Result<(), TypesError> {
        let ws_map = self.workspaces.read().await;
        if !ws_map.contains_key(id) {
            return Err(TypesError::InternalError(format!(
                "Workspace {id} not found"
            )));
        }

        let mut current = self.current_workspace.write().await;
        *current = Some(id.to_string());

        // Persist active ID
        self.store
            .set("sys", "sys.current_workspace", id.as_bytes())
            .await?;

        Ok(())
    }

    /// Get the currently active workspace.
    pub async fn get_current(&self) -> Option<Workspace> {
        let current_id = self.current_workspace.read().await.clone();
        if let Some(id) = current_id {
            let ws_map = self.workspaces.read().await;
            ws_map.get(&id).cloned()
        } else {
            None
        }
    }

    /// List all defined workspaces.
    pub async fn list_workspaces(&self) -> Vec<Workspace> {
        let ws_map = self.workspaces.read().await;
        ws_map.values().cloned().collect()
    }

    async fn persist_workspaces(&self) -> Result<(), TypesError> {
        let ws_map = self.workspaces.read().await;
        let ws_list: Vec<Workspace> = ws_map.values().cloned().collect();
        let data = serde_json::to_vec(&ws_list).map_err(|e| {
            TypesError::InternalError(format!("Failed to serialize workspaces: {e}"))
        })?;

        self.store.set("sys", "sys.workspaces", &data).await?;
        Ok(())
    }
}
