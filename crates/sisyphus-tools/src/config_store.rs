use sisyphus_types::Config;
use std::sync::Arc;
use std::sync::RwLock;
use tokio::time::{interval, Duration};

/// Hot-reloadable configuration store.
///
/// Loads a TOML config file on startup and polls its modification time
/// every `poll_interval` seconds. When the file changes, the shared
/// `Arc<RwLock<Config>>` is updated atomically in-place.
///
/// # Example
///
/// ```ignore
/// use sisyphus_tools::ConfigStore;
///
/// let (store, config) = ConfigStore::load("config.toml").await.unwrap();
/// // Start background reload task
/// tokio::spawn(async move { store.start_watching().await });
/// // Read current config via config.read().unwrap()
/// ```
pub struct ConfigStore {
    config: Arc<RwLock<Config>>,
    path: String,
    poll_interval: Duration,
}

impl ConfigStore {
    /// Load a config file and return a `ConfigStore` + shared `Config` reference.
    ///
    /// The `ConfigStore` must be polled (via `start_watching()` or manual
    /// `check_reload()`) for hot-reload to work.
    pub fn load(path: &str) -> Result<(Self, Arc<RwLock<Config>>), String> {
        let mut config = Config::from_file(path)?;
        config.apply_env_overrides();
        let arc = Arc::new(RwLock::new(config));
        Ok((
            Self {
                config: arc.clone(),
                path: path.to_string(),
                poll_interval: Duration::from_secs(3),
            },
            arc,
        ))
    }

    /// Set a custom polling interval (default: 3 seconds).
    pub fn set_poll_interval(&mut self, secs: u64) {
        self.poll_interval = Duration::from_secs(secs);
    }

    /// Create a ConfigStore from an existing shared config with no file watching.
    ///
    /// Useful for tests or when config file is unavailable — `check_reload()`
    /// will always return `false`.
    pub fn with_config(config: Arc<RwLock<Config>>) -> Self {
        Self {
            config,
            path: String::new(),
            poll_interval: Duration::from_secs(3),
        }
    }

    /// Read the current config from file and atomically update the shared reference.
    /// Returns `true` if a reload occurred.
    pub fn check_reload(&self) -> bool {
        match Config::from_file(&self.path) {
            Ok(mut new_config) => {
                new_config.apply_env_overrides();
                let mut cfg = self.config.write().unwrap();
                // Serialize both to check if content actually changed
                let old_json = serde_json::to_value(&*cfg).ok();
                let new_json = serde_json::to_value(&new_config).ok();
                if old_json != new_json {
                    *cfg = new_config;
                    return true;
                }
                false
            }
            Err(_) => false,
        }
    }

    /// Start the background polling loop.
    ///
    /// This never returns under normal operation. Spawn it as a background task:
    ///
    /// ```ignore
    /// tokio::spawn(async move { store.start_watching().await });
    /// ```
    pub async fn start_watching(&self) {
        let mut ticker = interval(self.poll_interval);
        loop {
            ticker.tick().await;
            self.check_reload();
        }
    }

    /// Get a clone of the shared config reference.
    pub fn config_ref(&self) -> Arc<RwLock<Config>> {
        self.config.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn test_config_store_load() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("test.toml");
        fs::write(
            &config_path,
            r#"
[cognitive_loop]
max_iterations = 5
convergence_threshold = 0.3
"#,
        )
        .unwrap();

        let (store, config) = ConfigStore::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 5);

        // Clean up
        drop(store);
        drop(config);
    }

    #[tokio::test]
    async fn test_config_store_reload() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("reload.toml");
        fs::write(
            &config_path,
            r#"
[cognitive_loop]
max_iterations = 5
convergence_threshold = 0.3
"#,
        )
        .unwrap();

        let (store, config) = ConfigStore::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 5);

        // Modify file
        fs::write(
            &config_path,
            r#"
[cognitive_loop]
max_iterations = 42
convergence_threshold = 0.3
"#,
        )
        .unwrap();

        // Need to wait a moment for fs timestamps to update
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(store.check_reload());
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 42);
    }

    #[tokio::test]
    async fn test_config_store_no_reload_on_same_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("same.toml");
        let content = r#"
[cognitive_loop]
max_iterations = 5
convergence_threshold = 0.3
"#;
        fs::write(&config_path, content).unwrap();

        let (store, config) = ConfigStore::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 5);

        // Write same content
        fs::write(&config_path, content).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should not report a reload (content hasn't changed)
        assert!(!store.check_reload());
    }
}
