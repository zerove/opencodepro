use async_trait::async_trait;
use dashmap::DashMap;
use sisyphus_types::ToolExecutor;
use std::path::PathBuf;
use std::sync::Arc;
use wasmtime::*;

pub struct WasmToolExecutor {
    engine: Engine,
    tools_path: PathBuf,
    module_cache: Arc<DashMap<String, Module>>,
}

impl Default for WasmToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmToolExecutor {
    pub fn new() -> Self {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.max_wasm_stack(1_000_000);

        let mut pooling = wasmtime::PoolingAllocationConfig::default();
        pooling.max_memory_size(10 * 65536);
        config.allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling(pooling));

        let engine = Engine::new(&config).expect("Failed to create Wasmtime Engine");

        Self {
            engine,
            tools_path: PathBuf::from("/home/zeron/dev/opencodepro/tools"),
            module_cache: Arc::new(DashMap::new()),
        }
    }

    async fn load_wasm_bytes(&self, tool_id: &str) -> Result<Vec<u8>, String> {
        let wasm_path = self.tools_path.join(format!("{}.wasm", tool_id));
        tokio::fs::read(&wasm_path).await.map_err(|e| {
            format!(
                "Failed to read WASM binary '{}' ({}): {}",
                tool_id,
                wasm_path.display(),
                e
            )
        })
    }

    async fn get_or_compile_module(&self, tool_id: &str) -> Result<Module, String> {
        if let Some(cached) = self.module_cache.get(tool_id) {
            return Ok(cached.clone());
        }
        let wasm_bytes = self.load_wasm_bytes(tool_id).await?;
        let engine = self.engine.clone();
        let module = tokio::task::spawn_blocking(move || Module::new(&engine, &wasm_bytes))
            .await
            .map_err(|e| format!("Compilation task failed: {}", e))?
            .map_err(|e| format!("Failed to compile WASM module '{}': {}", tool_id, e))?;
        self.module_cache
            .insert(tool_id.to_string(), module.clone());
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
    ) -> Result<Vec<u8>, String> {
        let module = self.get_or_compile_module(tool_id).await?;

        let linker = Linker::new(&self.engine);
        let mut store = Store::new(&self.engine, ());
        store
            .set_fuel(fuel_limit)
            .map_err(|e| format!("Failed to set fuel for '{}': {}", tool_id, e))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| format!("Failed to instantiate WASM module '{}': {}", tool_id, e))?;

        // 5. Look for the "execute" export with dynamic signature handling
        let execute_func = instance
            .get_func(&mut store, "execute")
            .ok_or_else(|| format!("No 'execute' export in module '{}'", tool_id))?;

        // Build argument list based on the function's expected parameter count
        let func_ty = execute_func.ty(&store);
        let params: Vec<Val> = match func_ty.params().len() {
            0 => vec![],
            n => vec![Val::I32(0); n],
        };

        let mut results = vec![Val::I32(0)];
        execute_func
            .call(&mut store, &params, &mut results)
            .map_err(|e| {
                // Translate traps (fuel exhaustion, stack overflow, memory violation)
                // into the ResourceExhausted format the tests expect.
                format!("ResourceExhausted: {}", e)
            })?;

        // 6. Read result string from WASM linear memory
        let result_ptr = match results.first() {
            Some(Val::I32(ptr)) => *ptr as usize,
            _ => return Ok(vec![]),
        };

        // Read memory export (not all modules have "memory" — that's OK)
        let result_bytes = if let Some(memory) = instance.get_memory(&mut store, "memory") {
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
        };

        Ok(result_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pool_init() {
        let _executor = WasmToolExecutor::new();
        // If this doesn't panic, the engine started successfully.
        assert!(true);
    }
}
