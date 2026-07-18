//! Skill Registry client for the Sisyphus ecosystem.
//! Handles remote discovery, downloading, and installation of cognitive skills.

use async_trait::async_trait;
use reqwest::Client;
use sisyphus_mem::skill_store::SkillStore;
use sisyphus_types::skill::{SkillBundle, SkillMetadata};
use sisyphus_types::ExecutorRouter;
use sisyphus_types::TypesError;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] TypesError),
    #[error("Dependency missing: tool {0} is required by this skill but not installed")]
    DependencyMissing(String),
    #[error("Invalid bundle: {0}")]
    InvalidBundle(String),
    #[error("Skill already installed: {0}")]
    AlreadyInstalled(String),
}

#[async_trait]
pub trait SkillRegistry: Send + Sync {
    /// Fetch the list of available skills from the remote registry.
    async fn fetch_available_skills(&self) -> Result<Vec<SkillMetadata>, RegistryError>;

    /// Download and install a skill bundle by its ID.
    async fn install_skill(&self, skill_id: &str) -> Result<(), RegistryError>;

    /// List all skills currently installed locally.
    async fn list_installed_skills(&self) -> Result<Vec<SkillMetadata>, RegistryError>;

    /// Uninstall a skill by its ID.
    async fn uninstall_skill(&self, skill_id: &str) -> Result<(), RegistryError>;

    /// Check if a skill is currently installed locally.
    async fn is_installed(&self, skill_id: &str) -> Result<bool, RegistryError>;

    /// Retrieve a full skill bundle if installed locally.
    async fn get_skill(&self, skill_id: &str) -> Result<Option<SkillBundle>, RegistryError>;
}

pub struct RemoteSkillRegistry {
    registry_url: String,
    http_client: Client,
    store: Arc<SkillStore>,
    router: Arc<ExecutorRouter>,
}

impl RemoteSkillRegistry {
    pub fn new(registry_url: String, store: Arc<SkillStore>, router: Arc<ExecutorRouter>) -> Self {
        Self {
            registry_url,
            http_client: Client::new(),
            store,
            router,
        }
    }
}

#[async_trait]
impl SkillRegistry for RemoteSkillRegistry {
    async fn fetch_available_skills(&self) -> Result<Vec<SkillMetadata>, RegistryError> {
        let url = format!("{}/skills", self.registry_url);
        let skills = self
            .http_client
            .get(url)
            .send()
            .await?
            .json::<Vec<SkillMetadata>>()
            .await?;

        Ok(skills)
    }

    async fn install_skill(&self, skill_id: &str) -> Result<(), RegistryError> {
        // 1. Download the bundle
        let url = format!("{}/skills/{}", self.registry_url, skill_id);
        let bundle = self
            .http_client
            .get(url)
            .send()
            .await?
            .json::<SkillBundle>()
            .await?;

        // 2. Dependency Pre-check
        // Ensure all required tools are present in the ExecutorRouter
        for tool_id in &bundle.required_tools {
            if !self
                .router
                .has_tool(tool_id)
                .await
                .map_err(RegistryError::Storage)?
            {
                return Err(RegistryError::DependencyMissing(tool_id.clone()));
            }
        }

        // 3. Persistence
        self.store.install(bundle).await?;

        tracing::info!(skill_id = %skill_id, "Successfully installed skill from remote registry");
        Ok(())
    }

    async fn list_installed_skills(&self) -> Result<Vec<SkillMetadata>, RegistryError> {
        self.store.list().await.map_err(RegistryError::Storage)
    }

    async fn uninstall_skill(&self, skill_id: &str) -> Result<(), RegistryError> {
        self.store
            .uninstall(skill_id)
            .await
            .map_err(RegistryError::Storage)
    }

    async fn is_installed(&self, skill_id: &str) -> Result<bool, RegistryError> {
        Ok(self.store.get(skill_id).await?.is_some())
    }

    async fn get_skill(&self, skill_id: &str) -> Result<Option<SkillBundle>, RegistryError> {
        self.store
            .get(skill_id)
            .await
            .map_err(RegistryError::Storage)
    }
}
