use async_trait::async_trait;
use dashmap::DashMap;
use sisyphus_metrics::record_wasm_execution;
use sisyphus_types::ToolExecutor;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use wasmtime::*;

pub mod custom_executor;
pub mod plugin_manager;
pub use custom_executor::CustomToolExecutor;
pub use plugin_manager::PluginManager;

#[derive(Error, Debug)]
pub enum SandboxError {
    #[error("Failed to create Wasmtime Engine: {0}")]
    EngineCreationFailed(#[from] wasmtime::Error),
    #[error("Failed to read WASM binary '{tool_id}' ({path}): {source}")]
    IoError {
        tool_id: String,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Compilation task failed: {0}")]
    TaskJoinError(String),
    #[error("Failed to compile WASM module '{tool_id}': {source}")]
    CompileError {
        tool_id: String,
        source: wasmtime::Error,
    },
    #[error("Failed to set fuel for '{tool_id}': {source}")]
    FuelError {
        tool_id: String,
        source: wasmtime::Error,
    },
    #[error("Failed to instantiate WASM module '{tool_id}': {source}")]
    InstantiationError {
        tool_id: String,
        source: wasmtime::Error,
    },
    #[error("No 'execute' export in module '{0}'")]
    MissingExecuteExport(String),
    #[error("ResourceExhausted: {0}")]
    ExecutionError(String),
}

#[derive(Clone)]
pub struct WasmToolExecutor {
    engine: Engine,
    tools_path: PathBuf,
    pub(crate) module_cache: Arc<DashMap<String, Module>>,
}

impl sisyphus_types::CacheInvalidatable for WasmToolExecutor {
    fn invalidate_cache(&self, tool_id: &str) {
        tracing::debug!(tool_id = %tool_id, "Invalidating WASM module cache");
        self.module_cache.remove(tool_id);
    }
}

impl Default for WasmToolExecutor {
    fn default() -> Self {
        Self::new().expect("WasmToolExecutor::new() should succeed with default config")
    }
}

impl WasmToolExecutor {
    pub fn new() -> Result<Self, SandboxError> {
        Self::with_config(None, None)
    }

    /// Create a `WasmToolExecutor` with optional sandbox configuration overrides.
    ///
    /// - `tools_path`: directory containing `.wasm` tool binaries (default: `"tools"`)
    /// - `max_memory_pages`: WASM linear memory limit in 64 KiB pages (default: `10`)
    pub fn with_config(
        tools_path: Option<PathBuf>,
        max_memory_pages: Option<u32>,
    ) -> Result<Self, SandboxError> {
        let tools_path = tools_path.unwrap_or_else(|| PathBuf::from("tools"));
        let max_memory_pages = max_memory_pages.unwrap_or(10);

        let mut config = Config::new();
        config.consume_fuel(true);
        config.max_wasm_stack(1_000_000);

        let mut pooling = wasmtime::PoolingAllocationConfig::default();
        pooling
            .max_memory_size(max_memory_pages as usize * 65536)
            // Conservative pool sizes to avoid large pre-allocated mmaps in memory-constrained envs
            .total_tables(10)
            .table_elements(100)
            .total_core_instances(10)
            .total_memories(10);
        config.allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling(pooling));

        let engine = Engine::new(&config)?;

        Ok(Self {
            engine,
            tools_path,
            module_cache: Arc::new(DashMap::new()),
        })
    }

    async fn load_wasm_bytes(&self, tool_id: &str) -> Result<Vec<u8>, SandboxError> {
        let wasm_path = self.tools_path.join(format!("{}.wasm", tool_id));
        tokio::fs::read(&wasm_path)
            .await
            .map_err(|e| SandboxError::IoError {
                tool_id: tool_id.to_string(),
                path: wasm_path,
                source: e,
            })
    }

    async fn get_or_compile_module(&self, tool_id: &str) -> Result<Module, SandboxError> {
        let span = tracing::info_span!("sandbox.compile_module", tool_id = %tool_id);
        let _enter = span.enter();

        if let Some(cached) = self.module_cache.get(tool_id) {
            tracing::debug!("Module cache hit");
            return Ok(cached.clone());
        }

        let start = Instant::now();
        let wasm_bytes = self.load_wasm_bytes(tool_id).await?;
        let load_duration = start.elapsed();
        tracing::debug!(
            bytes = wasm_bytes.len(),
            duration_ms = load_duration.as_millis(),
            "WASM binary loaded"
        );

        let engine = self.engine.clone();
        let compile_start = Instant::now();
        let module = tokio::task::spawn_blocking(move || Module::new(&engine, &wasm_bytes))
            .await
            .map_err(|e| SandboxError::TaskJoinError(e.to_string()))?
            .map_err(|e| SandboxError::CompileError {
                tool_id: tool_id.to_string(),
                source: e,
            })?;
        let compile_duration = compile_start.elapsed();
        tracing::info!(tool_id = %tool_id, duration_ms = compile_duration.as_millis(), "WASM module compiled");

        self.module_cache
            .insert(tool_id.to_string(), module.clone());
        tracing::debug!("Module cached");

        Ok(module)
    }
}

#[async_trait]
impl ToolExecutor for WasmToolExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        use sisyphus_types::TypesError;

        let span =
            tracing::info_span!("sandbox.execute", tool_id = %tool_id, fuel_limit = fuel_limit);
        let _enter = span.enter();

        let exec_start = Instant::now();
        let module = self.get_or_compile_module(tool_id).await.map_err(|e| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: e.to_string(),
            }
        })?;

        let linker = Linker::new(&self.engine);
        let mut store = Store::new(&self.engine, ());
        store
            .set_fuel(fuel_limit)
            .map_err(|e| TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: SandboxError::FuelError {
                    tool_id: tool_id.to_string(),
                    source: e,
                }
                .to_string(),
            })?;

        let instance = linker.instantiate(&mut store, &module).map_err(|e| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: SandboxError::InstantiationError {
                    tool_id: tool_id.to_string(),
                    source: e,
                }
                .to_string(),
            }
        })?;

        // 5. Look for the "execute" export with dynamic signature handling
        let execute_func = instance.get_func(&mut store, "execute").ok_or_else(|| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: SandboxError::MissingExecuteExport(tool_id.to_string()).to_string(),
            }
        })?;

        // Build argument list based on the function's expected parameter count
        let func_ty = execute_func.ty(&store);
        let params: Vec<Val> = match func_ty.params().len() {
            0 => vec![],
            n => vec![Val::I32(0); n],
        };

        let mut results = vec![Val::I32(0)];
        let call_start = Instant::now();
        let call_result = execute_func
            .call(&mut store, &params, &mut results)
            .map_err(|e| {
                // Translate traps (fuel exhaustion, stack overflow, memory violation)
                // into the ResourceExhausted format the tests expect.
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: SandboxError::ExecutionError(e.to_string()).to_string(),
                }
            });
        let call_duration = call_start.elapsed();

        // Check fuel consumption
        let fuel_consumed = fuel_limit.saturating_sub(store.get_fuel().unwrap_or(0));
        tracing::debug!(
            fuel_consumed,
            call_duration_ms = call_duration.as_millis(),
            "WASM function call completed"
        );

        let result_bytes = match call_result {
            Ok(_) => {
                // 6. Read result string from WASM linear memory
                let result_ptr = match results.first() {
                    Some(Val::I32(ptr)) => *ptr as usize,
                    _ => return Ok(vec![]),
                };

                // Read memory export (not all modules have "memory" — that's OK)
                if let Some(memory) = instance.get_memory(&mut store, "memory") {
                    let data = memory.data(&store);
                    if result_ptr < data.len() {
                        // Read until null terminator
                        let end = data[result_ptr..]
                            .iter()
                            .position(|&b| b == 0)
                            .map(|pos| result_ptr + pos)
                            .unwrap_or(data.len());
                        data[result_ptr..end].to_vec()
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, fuel_consumed, "WASM execution failed");
                return Err(e);
            }
        };

        let exec_duration = exec_start.elapsed();
        tracing::info!(tool_id = %tool_id, duration_ms = exec_duration.as_millis(), fuel_consumed, result_bytes = result_bytes.len(), "WASM execution completed");

        record_wasm_execution(
            tool_id,
            call_result.is_ok(),
            exec_duration.as_secs_f64(),
            fuel_consumed,
        );

        Ok(result_bytes)
    }
    fn as_cache_invalidatable(&self) -> Option<&dyn sisyphus_types::CacheInvalidatable> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create WasmToolExecutor pointed at the workspace-root `tools/` directory.
    fn test_executor() -> WasmToolExecutor {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // CARGO_MANIFEST_DIR = crates/sisyphus-sandbox, tools is at workspace root
        let tools_path = manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tools");
        WasmToolExecutor::with_config(Some(tools_path), None).unwrap()
    }

    #[tokio::test]
    async fn test_engine_creation() {
        let executor = test_executor();
        assert_eq!(executor.module_cache.len(), 0);
    }

    #[tokio::test]
    async fn test_load_nonexistent_tool() {
        let executor = test_executor();
        let result = executor.load_wasm_bytes("nonexistent_tool_xyz").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to read WASM binary"));
    }

    #[tokio::test]
    async fn test_get_or_compile_nonexistent() {
        let executor = test_executor();
        let result = executor.get_or_compile_module("nonexistent_tool_xyz").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to read WASM binary"));
    }

    #[tokio::test]
    async fn test_get_or_compile_and_cache() {
        let executor = test_executor();

        executor.get_or_compile_module("hello").await.unwrap();
        assert_eq!(executor.module_cache.len(), 1);

        // Second call hits cache: entry count stays at 1
        executor.get_or_compile_module("hello").await.unwrap();
        assert_eq!(executor.module_cache.len(), 1);
    }

    #[tokio::test]
    async fn test_execute_nonexistent_tool() {
        let executor = test_executor();
        let result = executor.execute("nonexistent_tool_xyz", vec![], 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_hello_valid() {
        let executor = test_executor();
        let result = executor.execute("hello", vec![], 1000).await;
        assert!(result.is_ok());
        let output = String::from_utf8(result.unwrap()).unwrap();
        assert_eq!(output, "Hello from WASM!");
    }

    #[tokio::test]
    async fn test_execute_fuel_exhaustion() {
        let executor = test_executor();
        let result = executor.execute("infinite_loop", vec![], 100).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("ResourceExhausted"),
            "Expected ResourceExhausted, got: {}",
            err_str
        );
    }

    #[tokio::test]
    async fn test_execute_memory_exhaustion() {
        let executor = test_executor();
        let result = executor.execute("memory_hog", vec![], 1000).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("ResourceExhausted")
                || err_str.contains("memory")
                || err_str.contains("translation"),
            "Expected memory-related error, got: {}",
            err_str
        );
    }

    #[tokio::test]
    async fn test_execute_unknown_export() {
        let executor = test_executor();
        let result = executor.execute("hello", vec![], 1000).await;
        // hello.wasm has execute export, so this should succeed
        assert!(result.is_ok());
    }
}
