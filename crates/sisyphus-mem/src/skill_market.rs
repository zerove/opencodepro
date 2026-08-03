//! Skill Market - Remote skill discovery and installation.
//! Allows discovering, downloading, and installing skills from remote registries.

use serde::{Deserialize, Serialize};
use sisyphus_types::skill::{SkillBundle, SkillMetadata};

/// Remote skill registry entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRegistryEntry {
    pub metadata: SkillMetadata,
    pub download_url: String,
    pub checksum: String,
    pub size_bytes: u64,
    pub downloads: u64,
    pub rating: f32,
}

/// Skill market configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMarketConfig {
    /// List of registry URLs to search
    pub registries: Vec<String>,
    /// Cache directory for downloaded skills
    pub cache_dir: String,
    /// Auto-update skills on startup
    pub auto_update: bool,
}

impl Default for SkillMarketConfig {
    fn default() -> Self {
        Self {
            registries: vec!["https://registry.sisyphus.dev".to_string()],
            cache_dir: "~/.sisyphus/skills/cache".to_string(),
            auto_update: false,
        }
    }
}

/// Skill market client for discovering and installing skills
pub struct SkillMarket {
    config: SkillMarketConfig,
    http_client: reqwest::Client,
}

impl SkillMarket {
    /// Create a new skill market client
    pub fn new(config: SkillMarketConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::new(),
        }
    }

    /// Search for skills in registries
    pub async fn search(&self, query: &str) -> Result<Vec<SkillRegistryEntry>, SkillMarketError> {
        let mut results = Vec::new();

        for registry_url in &self.config.registries {
            let url = format!("{}/api/v1/skills/search?q={}", registry_url, query);

            match self.http_client.get(&url).send().await {
                Ok(response) => {
                    if let Ok(entries) = response.json::<Vec<SkillRegistryEntry>>().await {
                        results.extend(entries);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to search registry {}: {}", registry_url, e);
                }
            }
        }

        Ok(results)
    }

    /// Get skill details from registry
    pub async fn get_skill(
        &self,
        skill_id: &str,
    ) -> Result<Option<SkillRegistryEntry>, SkillMarketError> {
        for registry_url in &self.config.registries {
            let url = format!("{}/api/v1/skills/{}", registry_url, skill_id);

            match self.http_client.get(&url).send().await {
                Ok(response) => {
                    if let Ok(entry) = response.json::<SkillRegistryEntry>().await {
                        return Ok(Some(entry));
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to get skill from {}: {}", registry_url, e);
                }
            }
        }

        Ok(None)
    }

    /// Download a skill bundle
    pub async fn download(
        &self,
        entry: &SkillRegistryEntry,
    ) -> Result<SkillBundle, SkillMarketError> {
        let response = self
            .http_client
            .get(&entry.download_url)
            .send()
            .await
            .map_err(|e| SkillMarketError::DownloadFailed(e.to_string()))?;

        let bundle: SkillBundle = response
            .json()
            .await
            .map_err(|e| SkillMarketError::DownloadFailed(e.to_string()))?;

        // Verify checksum
        let content = serde_json::to_string(&bundle)
            .map_err(|e| SkillMarketError::DownloadFailed(e.to_string()))?;
        let actual_checksum = format!("{:x}", md5::compute(content.as_bytes()));

        if actual_checksum != entry.checksum {
            return Err(SkillMarketError::ChecksumMismatch {
                expected: entry.checksum.clone(),
                actual: actual_checksum,
            });
        }

        Ok(bundle)
    }

    /// List installed skills with update info
    pub async fn list_installed(
        &self,
        installed: &[SkillMetadata],
    ) -> Result<Vec<SkillUpdateInfo>, SkillMarketError> {
        let mut updates = Vec::new();

        for meta in installed {
            if let Ok(Some(remote)) = self.get_skill(&meta.id).await
                && remote.metadata.version != meta.version
            {
                updates.push(SkillUpdateInfo {
                    current_version: meta.version.clone(),
                    available_version: remote.metadata.version.clone(),
                    entry: remote,
                });
            }
        }

        Ok(updates)
    }

    /// Publish a skill bundle to a registry
    pub async fn publish(
        &self,
        registry_url: &str,
        bundle: &SkillBundle,
    ) -> Result<SkillRegistryEntry, SkillMarketError> {
        let url = format!("{}/api/v1/skills", registry_url);

        let response = self
            .http_client
            .post(&url)
            .json(bundle)
            .send()
            .await
            .map_err(|e| SkillMarketError::RegistryError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SkillMarketError::RegistryError(format!(
                "Publish failed with status: {}",
                response.status()
            )));
        }

        response
            .json::<SkillRegistryEntry>()
            .await
            .map_err(|e| SkillMarketError::RegistryError(e.to_string()))
    }

    /// Download a skill and install it into a SkillStore.
    pub async fn download_and_install(
        &self,
        entry: &SkillRegistryEntry,
        skill_store: &crate::skill_store::SkillStore,
    ) -> Result<(), SkillMarketError> {
        let bundle = self.download(entry).await?;
        skill_store
            .install(bundle)
            .await
            .map_err(|e| SkillMarketError::DownloadFailed(e.to_string()))?;
        Ok(())
    }

    /// Search for a skill and install the top result.
    pub async fn search_and_install(
        &self,
        query: &str,
        skill_store: &crate::skill_store::SkillStore,
    ) -> Result<(), SkillMarketError> {
        let results = self.search(query).await?;
        if results.is_empty() {
            return Err(SkillMarketError::NotFound(format!(
                "No skills found for query: {}",
                query
            )));
        }

        let best = results
            .into_iter()
            .max_by(|a, b| {
                a.rating
                    .partial_cmp(&b.rating)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .ok_or_else(|| SkillMarketError::NotFound(format!("No skills found for: {}", query)))?;

        self.download_and_install(&best, skill_store).await
    }
}

/// Information about available skill update
#[derive(Debug, Clone)]
pub struct SkillUpdateInfo {
    pub current_version: String,
    pub available_version: String,
    pub entry: SkillRegistryEntry,
}

/// Skill market errors
#[derive(Debug, thiserror::Error)]
pub enum SkillMarketError {
    #[error("Failed to download skill: {0}")]
    DownloadFailed(String),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Registry error: {0}")]
    RegistryError(String),

    #[error("Skill not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_config_default() {
        let config = SkillMarketConfig::default();
        assert_eq!(config.registries.len(), 1);
        assert!(!config.auto_update);
    }
}
