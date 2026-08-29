use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::WasmToolExecutor;
use async_trait::async_trait;
use sisyphus_types::PluginDescriptor;
use sisyphus_types::PluginManagerTrait;

pub struct PluginManager {
    executor: WasmToolExecutor,
    plugins: Arc<DashMap<String, PluginDescriptor>>,
}

impl PluginManager {
    /// Create a new plugin manager that uses `executor` to run plugin code.
    ///
    /// The manager is reference-counted (`Arc`) so the same instance can be
    /// shared across all SOP executors.
    pub fn new(executor: WasmToolExecutor) -> Arc<Self> {
        let plugins = Arc::new(DashMap::new());
        let manager = Arc::new(Self {
            executor,
            plugins: plugins.clone(),
        });

        // Spawn hot-reload polling loop
        let manager_clone = Arc::clone(&manager);
        tokio::spawn(async move {
            manager_clone.run_hot_reload_loop().await;
        });

        manager
    }

    /// Load a WASM plugin from `descriptor` and register it under its `name`.
    ///
    /// Replacing an existing plugin with the same name is allowed; the old
    /// instance is dropped after the new one is successfully loaded.
    ///
    /// # Errors
    /// Returns `SandboxError` if the WASM module fails to compile or the
    /// descriptor is missing required fields (name, entry point).
    pub async fn register_plugin(&self, descriptor: PluginDescriptor) -> Result<(), String> {
        debug!(plugin = %descriptor.name, "Registering plugin");

        // Calculate initial checksum if not provided
        let mut desc = descriptor;
        if desc.checksum.is_none() {
            desc.checksum = Some(self.calculate_checksum(&desc.wasm_path).await?);
        }

        self.plugins.insert(desc.name.clone(), desc);
        Ok(())
    }

    /// Unload a previously registered plugin by name.
    ///
    /// Returns an error string if no plugin with that name is registered.
    pub fn unload_plugin(&self, name: &str) -> Result<(), String> {
        debug!(plugin = %name, "Unloading plugin");
        if let Some((_, desc)) = self.plugins.remove(name) {
            // Invalidate cache in executor
            self.executor.module_cache.remove(&desc.tool_id);
            info!(plugin = %name, tool_id = %desc.tool_id, "Plugin unloaded and cache invalidated");
        }
        Ok(())
    }

    /// Return a snapshot of all currently registered plugin descriptors.
    pub fn list_plugins(&self) -> Vec<PluginDescriptor> {
        self.plugins.iter().map(|r| r.value().clone()).collect()
    }
}

#[async_trait]
impl PluginManagerTrait for PluginManager {
    async fn register_plugin(&self, descriptor: PluginDescriptor) -> Result<(), String> {
        self.register_plugin(descriptor).await
    }

    fn unload_plugin(&self, name: &str) -> Result<(), String> {
        self.unload_plugin(name)
    }

    fn list_plugins(&self) -> Vec<PluginDescriptor> {
        self.list_plugins()
    }
}

impl PluginManager {
    async fn calculate_checksum(&self, path: &PathBuf) -> Result<String, String> {
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| format!("Failed to read plugin file {:?}: {}", path, e))?;

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let result = hasher.finalize();
        Ok(hex::encode(result))
    }

    async fn run_hot_reload_loop(&self) {
        let mut ticker = interval(Duration::from_secs(5));
        info!("Plugin hot-reload polling loop started (interval: 5s)");

        loop {
            ticker.tick().await;

            // We use a vec to avoid holding locks on the map while doing IO
            let plugins_to_check: Vec<PluginDescriptor> =
                self.plugins.iter().map(|r| r.value().clone()).collect();

            for desc in plugins_to_check {
                let path = desc.wasm_path.clone();
                match self.calculate_checksum(&path).await {
                    Ok(new_checksum) => {
                        if Some(new_checksum.clone()) != desc.checksum {
                            info!(
                                plugin = %desc.name,
                                tool_id = %desc.tool_id,
                                old_checksum = ?desc.checksum,
                                new_checksum = %new_checksum,
                                "Detected plugin change, hot-swapping module"
                            );

                            // 1. Update checksum in the registry
                            self.plugins.alter(&desc.name, |_, mut d| {
                                d.checksum = Some(new_checksum);
                                d
                            });

                            // 2. Invalidate the module cache in WasmToolExecutor
                            // This forces WasmToolExecutor to re-compile the module on next execution
                            self.executor.module_cache.remove(&desc.tool_id);
                        }
                    }
                    Err(e) => {
                        warn!(plugin = %desc.name, error = %e, "Failed to check plugin checksum");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    async fn setup_test_env() -> (Arc<PluginManager>, tempfile::TempDir) {
        let tmp_dir = tempdir().unwrap();
        let tools_path = tmp_dir.path().to_path_buf();

        let executor = WasmToolExecutor::with_config(Some(tools_path.clone()), None).unwrap();
        let manager = PluginManager::new(executor);

        (manager, tmp_dir)
    }

    #[tokio::test]
    async fn test_register_and_list_plugins() {
        let (manager, tmp_dir) = setup_test_env().await;
        let wasm_path = tmp_dir.path().join("test.wasm");
        fs::write(&wasm_path, b"wasm-binary-content").unwrap();

        let desc = PluginDescriptor {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            wasm_path,
            tool_id: "test_tool".to_string(),
            fuel_limit: 1000,
            checksum: None,
        };

        manager.register_plugin(desc).await.unwrap();
        let plugins = manager.list_plugins();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "test-plugin");
    }

    #[tokio::test]
    async fn test_unload_plugin() {
        let (manager, tmp_dir) = setup_test_env().await;
        let wasm_path = tmp_dir.path().join("test.wasm");
        fs::write(&wasm_path, b"wasm-binary-content").unwrap();

        let desc = PluginDescriptor {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            wasm_path,
            tool_id: "test_tool".to_string(),
            fuel_limit: 1000,
            checksum: None,
        };

        manager.register_plugin(desc).await.unwrap();
        assert_eq!(manager.list_plugins().len(), 1);

        manager.unload_plugin("test-plugin").unwrap();
        assert_eq!(manager.list_plugins().len(), 0);
    }

    #[tokio::test]
    async fn test_hot_reload_detection() {
        let (manager, tmp_dir) = setup_test_env().await;
        let wasm_path = tmp_dir.path().join("test.wasm");

        let wasm_v1 = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        fs::write(&wasm_path, &wasm_v1).unwrap();

        let desc = PluginDescriptor {
            name: "reload-plugin".to_string(),
            version: "1.0.0".to_string(),
            wasm_path: wasm_path.clone(),
            tool_id: "reload_tool".to_string(),
            fuel_limit: 1000,
            checksum: None,
        };

        manager.register_plugin(desc).await.unwrap();

        let module = wasmtime::Module::new(&manager.executor.engine, &wasm_v1).unwrap();
        manager
            .executor
            .module_cache
            .insert("reload_tool".to_string(), module);
        assert!(manager.executor.module_cache.contains_key("reload_tool"));

        let wasm_v2 = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x01];
        fs::write(&wasm_path, &wasm_v2).unwrap();

        tokio::time::sleep(Duration::from_secs(6)).await;

        assert!(
            !manager.executor.module_cache.contains_key("reload_tool"),
            "Cache should have been invalidated after file change"
        );
    }
}
