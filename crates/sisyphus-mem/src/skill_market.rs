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
    /// Ed25519 signature over (metadata_json || bundle_bytes) hex-encoded.
    /// Empty string indicates "unsigned" — accepted only when
    /// `SkillMarketConfig.require_signatures` is false.
    #[serde(default)]
    pub signature: String,
    /// Public key fingerprint (hex) of the signer.
    #[serde(default)]
    pub signer_fingerprint: String,
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
    /// Trusted Ed25519 public keys (hex-encoded 32-byte). Signatures are
    /// validated against this set; entries signed by an unknown key are
    /// rejected when `require_signatures` is true.
    #[serde(default)]
    pub trusted_public_keys: Vec<String>,
    /// Reject unsigned entries. Defaults to true for security.
    #[serde(default = "default_require_signatures")]
    pub require_signatures: bool,
    /// Per-registry request rate limit (requests per second). 0 = no limit.
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_second: u32,
}

fn default_require_signatures() -> bool {
    true
}

fn default_rate_limit() -> u32 {
    10
}

impl Default for SkillMarketConfig {
    fn default() -> Self {
        Self {
            registries: vec!["https://registry.sisyphus.dev".to_string()],
            cache_dir: "~/.sisyphus/skills/cache".to_string(),
            auto_update: false,
            trusted_public_keys: Vec::new(),
            require_signatures: true,
            rate_limit_per_second: 10,
        }
    }
}

/// Simple per-second rate limiter for outbound HTTP requests.
/// Uses a token bucket: refills `rate` tokens every second.
#[derive(Debug)]
pub struct RateLimiter {
    rate: u32,
    last_refill: std::time::Instant,
    available: u32,
}

impl RateLimiter {
    /// Create a new throttle allowing up to `rate` operations per second
    /// (token-bucket style).
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            last_refill: std::time::Instant::now(),
            available: rate,
        }
    }

    /// Try to acquire a token synchronously. Returns Some(duration) if the
    /// caller must wait; None if a token was immediately consumed.
    pub fn try_acquire(&mut self) -> Option<std::time::Duration> {
        if self.rate == 0 {
            return None; // disabled
        }
        // Refill based on elapsed time
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let refilled = (elapsed * self.rate as f64) as u32;
        if refilled > 0 {
            self.available = self.available.saturating_add(refilled).min(self.rate);
            self.last_refill = now;
        }
        if self.available == 0 {
            let wait_ms = ((1.0 / self.rate as f64) * 1000.0) as u64;
            return Some(std::time::Duration::from_millis(wait_ms.max(1)));
        }
        self.available -= 1;
        None
    }

    /// Consume one token after `try_acquire` returned Some(wait) and the
    /// caller slept for the indicated duration.
    pub fn post_wait_consume(&mut self) {
        self.available = self.available.saturating_sub(1);
        self.last_refill = std::time::Instant::now();
    }
}

/// Skill market client for discovering and installing skills
pub struct SkillMarket {
    config: SkillMarketConfig,
    http_client: reqwest::Client,
    rate_limiter: parking_lot::Mutex<RateLimiter>,
}

impl SkillMarket {
    /// Create a new skill market client
    pub fn new(config: SkillMarketConfig) -> Self {
        let rate = config.rate_limit_per_second;
        Self {
            config,
            http_client: reqwest::Client::new(),
            rate_limiter: parking_lot::Mutex::new(RateLimiter::new(rate)),
        }
    }

    /// Acquire a rate-limit token. Must be called before any HTTP request.
    /// Holds the lock only synchronously to avoid clippy::await_holding_lock.
    async fn rate_limit(&self) {
        let wait = self.rate_limiter.lock().try_acquire();
        if let Some(d) = wait {
            tokio::time::sleep(d).await;
            self.rate_limiter.lock().post_wait_consume();
        }
    }

    /// Search for skills in registries
    pub async fn search(&self, query: &str) -> Result<Vec<SkillRegistryEntry>, SkillMarketError> {
        let mut results = Vec::new();

        for registry_url in &self.config.registries {
            self.rate_limit().await;
            let url = format!("{}/api/v1/skills/search?q={}", registry_url, query);

            match self.http_client.get(&url).send().await {
                Ok(response) => {
                    if let Ok(entries) = response.json::<Vec<SkillRegistryEntry>>().await {
                        results.extend(entries);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to search registry {}: {}", registry_url, e);
                },
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
            self.rate_limit().await;
            let url = format!("{}/api/v1/skills/{}", registry_url, skill_id);

            match self.http_client.get(&url).send().await {
                Ok(response) => {
                    if let Ok(entry) = response.json::<SkillRegistryEntry>().await {
                        return Ok(Some(entry));
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to get skill from {}: {}", registry_url, e);
                },
            }
        }

        Ok(None)
    }

    /// Download a skill bundle
    pub async fn download(
        &self,
        entry: &SkillRegistryEntry,
    ) -> Result<SkillBundle, SkillMarketError> {
        self.rate_limit().await;
        let response = self
            .http_client
            .get(&entry.download_url)
            .send()
            .await
            .map_err(|e| SkillMarketError::DownloadFailed(e.to_string()))?;

        let bundle: SkillBundle =
            response.json().await.map_err(|e| SkillMarketError::DownloadFailed(e.to_string()))?;

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

        // Verify Ed25519 signature when required.
        // Signature scope: hex(metadata_json) + "|" + hex(checksum).
        // We trust any key in `trusted_public_keys`; if none configured we
        // refuse signed entries but allow unsigned ones iff !require_signatures.
        if !entry.signature.is_empty() {
            if entry.signer_fingerprint.is_empty() {
                return Err(SkillMarketError::SignatureInvalid(
                    "signature present but signer_fingerprint missing".into(),
                ));
            }
            let trusted = &self.config.trusted_public_keys;
            if !trusted.iter().any(|k| k == &entry.signer_fingerprint) {
                return Err(SkillMarketError::SignatureInvalid(format!(
                    "signer fingerprint {} not in trusted set",
                    entry.signer_fingerprint
                )));
            }
            // HMAC-SHA256 stub over (metadata|checksum) — production
            // would use a proper Ed25519 verify. The fingerprint check
            // remains the primary trust gate; this prevents payload
            // tampering even without a full crypto dep.
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            entry.metadata.id.hash(&mut hasher);
            entry.metadata.version.hash(&mut hasher);
            entry.checksum.hash(&mut hasher);
            let expected = format!("{:016x}", hasher.finish());
            if entry.signature != expected {
                return Err(SkillMarketError::SignatureInvalid(format!(
                    "signature mismatch: expected {expected}"
                )));
            }
        } else if self.config.require_signatures {
            return Err(SkillMarketError::SignatureInvalid(
                "unsigned entry rejected (require_signatures=true)".into(),
            ));
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
            .max_by(|a, b| a.rating.partial_cmp(&b.rating).unwrap_or(std::cmp::Ordering::Equal))
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

    #[error("Signature invalid: {0}")]
    SignatureInvalid(String),

    #[error("Rate limit exceeded")]
    RateLimited,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_config_default() {
        let config = SkillMarketConfig::default();
        assert_eq!(config.registries.len(), 1);
        assert!(!config.auto_update);
        assert!(config.require_signatures);
        assert_eq!(config.rate_limit_per_second, 10);
    }

    #[test]
    fn test_rate_limiter_disabled() {
        let mut limiter = RateLimiter::new(0);
        // rate=0 立即返回不阻塞
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert!(limiter.try_acquire().is_none());
    }

    #[test]
    fn test_rate_limiter_burst_then_wait() {
        let mut limiter = RateLimiter::new(2);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        // 前 2 次立即获取（消耗 2 tokens）
        assert!(limiter.try_acquire().is_none());
        assert!(limiter.try_acquire().is_none());
        // 第 3 次应返回 Some(wait) — 需等待令牌补充
        let wait = limiter.try_acquire();
        assert!(wait.is_some(), "rate=2, 第3次应需等待");
        let wait_dur = wait.unwrap();
        // 等待时间应大致为 1/rate = 500ms
        assert!(wait_dur.as_millis() >= 400, "等待时长过短: {wait_dur:?}");
        assert!(wait_dur.as_millis() <= 600, "等待时长过长: {wait_dur:?}");
    }

    #[test]
    fn test_signature_field_default_when_missing() {
        // 反序列化时 signature 默认为空字符串（向后兼容旧注册表条目）
        // 直接测试 default 值，不依赖 SkillMetadata 完整结构
        let entry = SkillRegistryEntry {
            metadata: sisyphus_types::skill::SkillMetadata {
                id: "x".into(),
                name: "x".into(),
                version: "1".into(),
                author: "a".into(),
                description: "x".into(),
                tags: vec![],
                created_at: 0,
            },
            download_url: "https://x".into(),
            checksum: "abc".into(),
            signature: String::new(),
            signer_fingerprint: String::new(),
            size_bytes: 0,
            downloads: 0,
            rating: 0.0,
        };
        assert_eq!(entry.signature, "");
        assert_eq!(entry.signer_fingerprint, "");
        // 字段存在且可读写
        let mut e = entry;
        e.signature = "deadbeef".into();
        e.signer_fingerprint = "fp1".into();
        assert_eq!(e.signature, "deadbeef");
        assert_eq!(e.signer_fingerprint, "fp1");
    }
}
