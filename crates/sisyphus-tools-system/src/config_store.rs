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

    /// 串行化**所有会触发 `apply_env_overrides()` 的用例**，而不仅仅是设置 env
    /// 的那一个。
    ///
    /// `ConfigStore::load` 与 `check_reload` 内部都会调用
    /// `apply_env_overrides()`，也就是**读**进程全局环境变量。因此任何一个
    /// 设置 `SISYPHUS_*` 的用例都会波及并行运行的其它用例，哪怕后者用的是自己
    /// 的临时目录、从未触碰那个变量。
    ///
    /// 原先只有 `test_config_store_reload_env_override_preserved` 持这把锁，
    /// 于是它设的 `..._MAX_ITERATIONS=99` 会漏进期望文件值 5 的
    /// `test_config_store_reload` / `..._no_reload_on_same_content` /
    /// `..._corrupted_then_fixed`，断言拿到 `left: 99, right: 5` 而失败
    /// （实测 40 次运行失败 6 次，约 15%）。
    ///
    /// 凡是走 `ConfigStore::load` 或 `check_reload` 的用例都必须取它。
    ///
    /// 用 `tokio::sync::Mutex` 而非 `std::sync::Mutex`：这些用例是 `async` 的，
    /// 且内部有 `sleep().await`。持 `std::sync::MutexGuard` 跨 await 会触发
    /// clippy `await_holding_lock`，更重要的是它会阻塞整个 worker 线程。
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
        // 本用例会触发 apply_env_overrides()（读进程全局 env），
        // 必须与设置 SISYPHUS_* 的用例互斥。
        let _guard = ENV_LOCK.lock().await;
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
        // 本用例会触发 apply_env_overrides()（读进程全局 env），
        // 必须与设置 SISYPHUS_* 的用例互斥。
        let _guard = ENV_LOCK.lock().await;
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
        // 本用例会触发 apply_env_overrides()（读进程全局 env），
        // 必须与设置 SISYPHUS_* 的用例互斥。
        let _guard = ENV_LOCK.lock().await;
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
        // 本用例会触发 apply_env_overrides()（读进程全局 env），
        // 必须与设置 SISYPHUS_* 的用例互斥。
        let _guard = ENV_LOCK.lock().await;
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
        // 本用例会触发 apply_env_overrides()（读进程全局 env），
        // 必须与设置 SISYPHUS_* 的用例互斥。
        let _guard = ENV_LOCK.lock().await;
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
        // 本用例会触发 apply_env_overrides()（读进程全局 env），
        // 必须与设置 SISYPHUS_* 的用例互斥。
        let _guard = ENV_LOCK.lock().await;
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
        // 本用例会触发 apply_env_overrides()（读进程全局 env），
        // 必须与设置 SISYPHUS_* 的用例互斥。
        let _guard = ENV_LOCK.lock().await;
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
        // 本用例会触发 apply_env_overrides()（读进程全局 env），
        // 必须与设置 SISYPHUS_* 的用例互斥。
        let _guard = ENV_LOCK.lock().await;
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
        // 本用例会触发 apply_env_overrides()（读进程全局 env），
        // 必须与设置 SISYPHUS_* 的用例互斥。
        let _guard = ENV_LOCK.lock().await;
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

        let _guard = ENV_LOCK.lock().await;
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
        // 本用例会触发 apply_env_overrides()（读进程全局 env），
        // 必须与设置 SISYPHUS_* 的用例互斥。
        let _guard = ENV_LOCK.lock().await;
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

    /// 回归测试：设置 env 的用例与读取 env 的用例必须互斥。
    ///
    /// `ConfigStore::load` 与 `check_reload` 都会调用 `apply_env_overrides()`，
    /// 也就是**读**进程全局环境变量。于是任何「设置 SISYPHUS_* 变量」的用例都会
    /// 影响并行运行的其它用例，哪怕后者用自己的临时目录、完全不碰那个变量。
    ///
    /// 原先 `ENV_LOCK` 只有 `test_config_store_reload_env_override_preserved`
    /// 一个用例持有：它设了 `..._MAX_ITERATIONS=99`，而并发的
    /// `test_config_store_reload` / `..._no_reload_on_same_content` /
    /// `..._corrupted_then_fixed` 期望文件里的 5。窗口内它们的
    /// `apply_env_overrides()` 会读到 99，断言拿到 `left: 99, right: 5` 而失败。
    /// 实测 40 次运行失败 6 次（约 15%）。
    ///
    /// 这里用 `Barrier` 让「设 env」与「读 env」两侧同时起跑，把概率性竞争放大
    /// 成确定性失败。注意屏障必须在**取锁之前**会合，否则先到线程持锁等屏障、
    /// 后到线程卡在锁上，直接死锁。
    ///
    /// 用 `std::thread` 而非 `tokio::spawn`：本 crate 未启用 `rt-multi-thread`，
    /// 而单线程运行时下第一个任务会占住唯一线程等屏障、第二个任务永远轮不到
    /// 执行，直接死锁。`Barrier` 本就是同步原语，不该绕道 async。
    #[test]
    fn env_override_leak_does_not_reach_concurrent_readers() {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        // 写入侧：模拟 test_config_store_reload_env_override_preserved。
        let writer = {
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                // 用 `blocking_lock`：本用例是同步线程，无法 `.await`，但必须
                // 与异步用例取**同一把** `ENV_LOCK`，否则两者不互斥、这个回归
                // 测试就保护不了真实场景。
                let _guard = ENV_LOCK.blocking_lock();
                unsafe { std::env::set_var("SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS", "99") };
                let dir = tempfile::TempDir::new().unwrap();
                let p = dir.path().join("writer.toml");
                write_toml(&p, 5);
                // 立刻复制出标量值，不让 RwLock 借用跨 await。
                let (_store, cfg) = ConfigStore::load(p.to_str().unwrap()).unwrap();
                let got = cfg.read().unwrap().cognitive_loop.max_iterations;
                unsafe { std::env::remove_var("SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS") };
                got
            })
        };

        // 读取侧：期望文件里的 5，不应被写入侧的 env 污染。
        let reader = {
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let _guard = ENV_LOCK.blocking_lock();
                let dir = tempfile::TempDir::new().unwrap();
                let p = dir.path().join("reader.toml");
                write_toml(&p, 5);
                let (_store, cfg) = ConfigStore::load(p.to_str().unwrap()).unwrap();
                let got = cfg.read().unwrap().cognitive_loop.max_iterations;
                got
            })
        };

        assert_eq!(writer.join().unwrap(), 99, "写入侧应套用自己的 env 覆盖");
        assert_eq!(
            reader.join().unwrap(),
            5,
            "读取侧被写入侧的 env 污染了：SISYPHUS_* 变量是进程全局的，\
             设置与读取必须互斥——否则读到 left: 99, right: 5"
        );
    }
}
