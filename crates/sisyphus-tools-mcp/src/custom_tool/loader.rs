use crate::tool_registry::{ToolInfo, ToolRegistry, ToolSource};
use sisyphus_types::CustomToolDefinition;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, error, info};

/// Custom Tool Loader.
pub struct CustomToolLoader {
    tools_dir: PathBuf,
}

impl CustomToolLoader {
    /// New.
    pub fn new(tools_dir: &str) -> Self {
        Self {
            tools_dir: PathBuf::from(tools_dir),
        }
    }

    /// 扫描工具目录并加载所有 .toml 定义
    pub async fn load_and_register(
        &self,
        registry: &ToolRegistry,
    ) -> Result<Vec<CustomToolDefinition>, String> {
        if !self.tools_dir.exists() {
            debug!(
                "Custom tools directory {:?} does not exist, skipping",
                self.tools_dir
            );
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(&self.tools_dir)
            .await
            .map_err(|e| format!("Failed to read tools directory: {e}"))?;

        let mut loaded_defs = Vec::new();

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| format!("Error reading directory entry: {e}"))?
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                match self.load_tool(&path).await {
                    Ok(def) => {
                        // 将 CustomToolDefinition 注册到 ToolRegistry
                        registry.register(
                            &def.name,
                            ToolInfo {
                                name: def.name.clone(),
                                description: def.description.clone(),
                                parameters: Some(def.schema.clone()),
                                source: ToolSource::Wasm, // 标记为 WASM 来源
                            },
                        );
                        debug!("Loaded custom tool: {}", def.name);
                        loaded_defs.push(def);
                    }
                    Err(e) => {
                        error!("Failed to load custom tool from {:?}: {}", path, e);
                    }
                }
            }
        }

        info!(
            "Registered {} custom tools from {:?}",
            loaded_defs.len(),
            self.tools_dir
        );
        Ok(loaded_defs)
    }

    async fn load_tool(&self, path: &Path) -> Result<CustomToolDefinition, String> {
        let content = fs::read_to_string(path)
            .await
            .map_err(|e| format!("Failed to read file: {e}"))?;

        let def: CustomToolDefinition =
            toml::from_str(&content).map_err(|e| format!("TOML parse error in {path:?}: {e}"))?;

        // 验证 handler 文件是否存在
        if !def.handler.exists() {
            return Err(format!("Handler WASM file {:?} not found", def.handler));
        }

        Ok(def)
    }
}
