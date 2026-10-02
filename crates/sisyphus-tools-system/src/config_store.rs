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
/// ```no_run
/// use sisyphus_tools_system::config_store::ConfigStore;
///
/// // load 是同步的（返回 Result<(ConfigStore, Arc<RwLock<Config>>), String>），
/// // 此前示例误写成 .await，因为标了 ```ignore 从不编译而无人发现。
/// let (store, config) = ConfigStore::load("config.toml").unwrap();
/// // 热重载需要后台轮询任务
/// tokio::spawn(async move { store.start_watching().await });
/// // 读当前配置：config.read().unwrap()
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
        let mut config = Config::from_file(path).map_err(|e| e.to_string())?;
        config.apply_env_overrides();
        config.validate().map_err(|e| e.to_string())?;
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
                if let Err(e) = new_config.validate() {
                    tracing::warn!(
                        "Config validation failed during reload, keeping previous config: {e}"
                    );
                    return false;
                }
                let mut cfg =
                    self.config.write().unwrap_or_else(std::sync::PoisonError::into_inner);
                // Serialize both to check if content actually changed
                let old_json = serde_json::to_value(&*cfg).ok();
                let new_json = serde_json::to_value(&new_config).ok();
                if old_json != new_json {
                    *cfg = new_config;
                    return true;
                }
                false
            },
            Err(_) => false,
        }
    }

    /// Start the background polling loop.
    ///
    /// This never returns under normal operation. Spawn it as a background task:
    ///
    /// ```no_run
    /// # use sisyphus_tools_system::config_store::ConfigStore;
    /// # async fn demo(store: ConfigStore) {
    /// tokio::spawn(async move { store.start_watching().await });
    /// # }
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn write_toml(path: &std::path::Path, max_iterations: u64) {
        fs::write(
            path,
            format!(
                r#"
[cognitive_loop]
max_iterations = {max_iterations}
convergence_threshold = 0.3
"#,
            ),
        )
        .unwrap();
    }

    fn write_invalid_toml(path: &std::path::Path) {
        fs::write(path, "this is {\nnot [[valid [[] toml").unwrap();
    }

    #[tokio::test]
    async fn test_config_store_load() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("test.toml");
        write_toml(&config_path, 5);

        let (store, config) = ConfigStore::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 5);

        drop(store);
        drop(config);
    }

    #[tokio::test]
    async fn test_config_store_load_nonexistent_path() {
        let result = ConfigStore::load("/tmp/__nonexistent_sisyphus_config__/config.toml");
        match result {
            Err(err) => assert!(
                err.contains("Failed to read"),
                "error should mention I/O failure"
            ),
            Ok(_) => panic!("expected Err for non-existent path"),
        }
    }

    #[tokio::test]
    async fn test_config_store_load_malformed_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("bad.toml");
        write_invalid_toml(&path);

        let result = ConfigStore::load(path.to_str().unwrap());
        match result {
            Err(err) => assert!(
                err.contains("Failed to parse"),
                "error should mention parse failure"
            ),
            Ok(_) => panic!("expected Err for malformed TOML"),
        }
    }

    #[tokio::test]
    async fn test_config_store_reload() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("reload.toml");
        write_toml(&config_path, 5);

        let (store, config) = ConfigStore::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 5);

        // Modify file
        write_toml(&config_path, 42);
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
"#
        .to_string();
        fs::write(&config_path, &content).unwrap();

        let (store, config) = ConfigStore::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 5);

        // Write same content
        fs::write(&config_path, &content).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(!store.check_reload());
    }

    #[tokio::test]
    async fn test_config_store_multiple_reload_cycles() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("multi.toml");

        write_toml(&config_path, 5);
        let (store, config) = ConfigStore::load(config_path.to_str().unwrap()).unwrap();

        write_toml(&config_path, 10);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(store.check_reload());
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 10);

        write_toml(&config_path, 15);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(store.check_reload());
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 15);

        write_toml(&config_path, 15);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!store.check_reload());
    }

    #[tokio::test]
    async fn test_config_store_with_config_no_reload() {
        let config = Arc::new(RwLock::new(Config::default()));
        let store = ConfigStore::with_config(config.clone());
        assert_eq!(
            store.config_ref().read().unwrap().cognitive_loop.max_iterations,
            10
        );
        assert!(!store.check_reload(), "with_config should not reload");
    }

    #[tokio::test]
    async fn test_config_store_check_reload_deleted_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("deleted.toml");

        write_toml(&config_path, 5);
        let (store, _config) = ConfigStore::load(config_path.to_str().unwrap()).unwrap();

        fs::remove_file(&config_path).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(!store.check_reload());
    }

    #[tokio::test]
    async fn test_config_store_set_poll_interval() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("poll.toml");
        write_toml(&path, 5);

        let (mut store, _config) = ConfigStore::load(path.to_str().unwrap()).unwrap();
        assert_eq!(store.poll_interval, Duration::from_secs(3));

        store.set_poll_interval(10);
        assert_eq!(store.poll_interval, Duration::from_secs(10));

        store.set_poll_interval(1);
        assert_eq!(store.poll_interval, Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_config_store_with_config_clone_ref() {
        let config = Arc::new(RwLock::new(Config::default()));
        let store = ConfigStore::with_config(config.clone());

        let cloned = store.config_ref();
        assert!(Arc::ptr_eq(&config, &cloned));
    }

    #[tokio::test]
    async fn test_config_store_reload_env_override_preserved() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("env_reload.toml");
        write_toml(&config_path, 5);

        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS", "99") };

        let (_store, config) = ConfigStore::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(
            config.read().unwrap().cognitive_loop.max_iterations,
            99,
            "env override should be applied on load"
        );

        unsafe { std::env::remove_var("SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS") };
    }

    #[tokio::test]
    async fn test_config_store_with_config_custom() {
        let custom = Config {
            llm: sisyphus_types::LlmConfig {
                api_key: "test-key".into(),
                endpoint: "http://test:8080".into(),
                model: "test-model".into(),
                ..Default::default()
            },
            ..Config::default()
        };
        let config = Arc::new(RwLock::new(custom));
        let store = ConfigStore::with_config(config.clone());

        let ref_ = store.config_ref();
        let cfg = ref_.read().unwrap();
        assert_eq!(cfg.llm.api_key, "test-key");
        assert_eq!(cfg.llm.model, "test-model");
        drop(cfg);
    }

    #[tokio::test]
    async fn test_config_store_reload_corrupted_then_fixed() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("corrupt_recover.toml");
        write_toml(&config_path, 5);

        let (store, config) = ConfigStore::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 5);

        // Corrupt and attempt reload
        write_invalid_toml(&config_path);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !store.check_reload(),
            "corrupt file should not trigger reload"
        );

        // Still has old value
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 5);

        // Fix the file
        write_toml(&config_path, 42);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(store.check_reload(), "fixed file should trigger reload");
        assert_eq!(config.read().unwrap().cognitive_loop.max_iterations, 42);
    }
}
