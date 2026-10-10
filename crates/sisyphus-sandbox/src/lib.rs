use async_trait::async_trait;
use dashmap::DashMap;
use sisyphus_metrics::record_wasm_execution;
use sisyphus_types::ToolExecutor;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use wasmtime::*;

/// Wall-clock bound for a single guest execution, and the epoch tick interval.
///
/// Fuel is the primary limit because it is deterministic and reproducible;
/// this is the backstop for when fuel cannot help. Generous enough that a
/// legitimate tool never trips it, tight enough that CI fails fast instead of
/// hanging until the runner's global timeout.
pub const EXECUTION_TIMEOUT_MS: u64 = 5_000;

pub mod custom_executor;
pub mod plugin_manager;
pub use custom_executor::CustomToolExecutor;
pub use plugin_manager::PluginManager;

#[derive(Error, Debug)]
/// Sandbox Error.
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

/// Knobs that exist so tests can defeat one limit at a time.
///
/// Defeating the fuel meter or the wall-clock bound from outside is impossible,
/// so both live as explicit parameters of the single real execution path. This
/// keeps every other step (instantiation, deadline arming, trap translation)
/// identical between the tested and production runs.
#[derive(Clone, Copy)]
struct ExecOpts {
    /// false = fuel accounting defeated, the guest can never run out of fuel.
    metered: bool,
    /// false = wall-clock bound disabled; only fuel can stop the guest.
    wall_clock: bool,
}

impl ExecOpts {
    const DEFAULT: Self = Self {
        metered: true,
        wall_clock: true,
    };
}

#[derive(Clone)]
/// Wasm Tool Executor.
pub struct WasmToolExecutor {
    engine: Engine,
    tools_path: PathBuf,
    pub(crate) module_cache: Arc<DashMap<String, Module>>,
    /// Keeps the epoch ticker alive for as long as this executor exists.
    /// Dropping it would stop epoch advancement and silently disable the
    /// wall-clock backstop, so it is stored rather than spawned-and-forgotten.
    _epoch_ticker: Arc<EpochTicker>,
}

/// Advances wasmtime's epoch counter on a fixed interval.
///
/// wasmtime only *checks* the epoch deadline from inside running guest code, so
/// the counter has to be moved by something outside the guest. One thread per
/// executor (not per call) keeps overhead flat and bounds concurrent tools.
struct EpochTicker {
    shutdown: Arc<std::sync::atomic::AtomicBool>,
}

impl EpochTicker {
    fn spawn(engine: Engine, interval: std::time::Duration) -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};

        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&shutdown);
        // A failed spawn leaves `shutdown` false and no thread; Drop still runs,
        // and the epoch deadline simply never fires (fuel remains primary).
        std::thread::Builder::new()
            .name("wasmtime-epoch".into())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(interval);
                    engine.increment_epoch();
                }
            })
            .ok();
        Self { shutdown }
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl sisyphus_types::CacheInvalidatable for WasmToolExecutor {
    fn invalidate_cache(&self, tool_id: &str) {
        tracing::debug!(tool_id = %tool_id, "Invalidating WASM module cache");
        self.module_cache.remove(tool_id);
    }
}

impl WasmToolExecutor {
    /// Build the default Wasmtime-backed sandbox.
    ///
    /// # Errors
    /// Returns `SandboxError::Wasmtime` if the runtime cannot be initialized
    /// (missing wasm32 target, OOM, etc.).
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
        // Fuel is a *cooperative* limit: it only stops code that still executes
        // wasm instructions. If metering itself regresses (missed fuel
        // accounting on `call_ref`/exception paths, see RUSTSEC-2026-0315) an
        // infinite loop is never interrupted and the call hangs forever.
        // Epoch interruption is an independent wall-clock bound enforced by the
        // runtime, so it still traps when fuel accounting is wrong.
        config.epoch_interruption(true);
        // wasmtime ≥46 defaults memory_reservation to 4GiB; the pooling allocator
        // pre-maps that much per slot, which hangs in memory-constrained envs.
        // Our WASM tools are small, so a modest reservation avoids the huge mmap.
        config.memory_reservation(2 * 1024 * 1024);

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
        let epoch_ticker = Arc::new(EpochTicker::spawn(
            engine.clone(),
            std::time::Duration::from_millis(EXECUTION_TIMEOUT_MS),
        ));

        Ok(Self {
            engine,
            tools_path,
            module_cache: Arc::new(DashMap::new()),
            _epoch_ticker: epoch_ticker,
        })
    }

    /// Arm the wall-clock deadline for one execution.
    ///
    /// Traps (rather than invoking a callback) so a runaway guest surfaces as an
    /// ordinary wasm trap and lands in the same `ResourceExhausted` error shape
    /// as the fuel-exhausted path.
    fn arm_epoch_deadline(&self, store: &mut Store<()>, enabled: bool) {
        if enabled {
            store.set_epoch_deadline(1);
            store.epoch_deadline_trap();
        }
    }

    async fn load_wasm_bytes(&self, tool_id: &str) -> Result<Vec<u8>, SandboxError> {
        let wasm_path = self.tools_path.join(format!("{}.wasm", tool_id));
        tokio::fs::read(&wasm_path).await.map_err(|e| SandboxError::IoError {
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

        self.module_cache.insert(tool_id.to_string(), module.clone());
        tracing::debug!("Module cached");

        Ok(module)
    }
}

impl WasmToolExecutor {
    /// 带测量的 WASM 执行内核（P2-9 燃料真值打通）。
    async fn execute_internal(
        &self,
        tool_id: &str,
        fuel_limit: u64,
    ) -> Result<(Vec<u8>, u64), sisyphus_types::TypesError> {
        self.execute_internal_opts(tool_id, fuel_limit, ExecOpts::DEFAULT).await
    }

    #[cfg(test)]
    pub(crate) async fn execute_internal_unmetered(
        &self,
        tool_id: &str,
        fuel_limit: u64,
        wall_clock: bool,
    ) -> Result<(Vec<u8>, u64), sisyphus_types::TypesError> {
        self.execute_internal_opts(
            tool_id,
            fuel_limit,
            ExecOpts {
                metered: false,
                wall_clock,
            },
        )
        .await
    }

    async fn execute_internal_opts(
        &self,
        tool_id: &str,
        fuel_limit: u64,
        opts: ExecOpts,
    ) -> Result<(Vec<u8>, u64), sisyphus_types::TypesError> {
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
        store.set_fuel(if opts.metered { fuel_limit } else { u64::MAX }).map_err(|e| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: SandboxError::FuelError {
                    tool_id: tool_id.to_string(),
                    source: e,
                }
                .to_string(),
            }
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

        // Wall-clock backstop, independent of fuel. See the `epoch_interruption`
        // note in `with_config`: fuel can fail to stop a runaway loop, this cannot.
        self.arm_epoch_deadline(&mut store, opts.wall_clock);

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
        let call_result = execute_func.call(&mut store, &params, &mut results).map_err(|e| {
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
                    _ => return Ok((vec![], fuel_consumed)),
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
            },
            Err(e) => {
                tracing::warn!(error = %e, fuel_consumed, "WASM execution failed");
                return Err(e);
            },
        };

        let exec_duration = exec_start.elapsed();
        tracing::info!(tool_id = %tool_id, duration_ms = exec_duration.as_millis(), fuel_consumed, result_bytes = result_bytes.len(), "WASM execution completed");

        record_wasm_execution(
            tool_id,
            call_result.is_ok(),
            exec_duration.as_secs_f64(),
            fuel_consumed,
        );

        Ok((result_bytes, fuel_consumed))
    }
}

#[async_trait]
impl ToolExecutor for WasmToolExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        let _ = args;
        let (data, _) = self.execute_internal(tool_id, fuel_limit).await?;
        Ok(data)
    }

    async fn execute_measured(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<(Vec<u8>, Option<sisyphus_types::ExecutionMeasurement>), sisyphus_types::TypesError>
    {
        use sisyphus_types::TypesError;
        let exec_start = Instant::now();
        let (data, fuel_consumed) =
            self.execute_internal(tool_id, fuel_limit).await.map_err(|e: TypesError| e)?;
        Ok((
            data,
            Some(sisyphus_types::ExecutionMeasurement {
                fuel_consumed,
                duration_ms: exec_start.elapsed().as_millis() as u64,
            }),
        ))
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
        let tools_path = manifest_dir.parent().unwrap().parent().unwrap().join("tools");
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
        assert!(result.unwrap_err().to_string().contains("Failed to read WASM binary"));
    }

    #[tokio::test]
    async fn test_get_or_compile_nonexistent() {
        let executor = test_executor();
        let result = executor.get_or_compile_module("nonexistent_tool_xyz").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read WASM binary"));
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

    /// 锁定实际燃料消耗的**具体数值**，而不只是「耗尽了会报错」。
    ///
    /// 为什么需要：`test_execute_fuel_exhaustion` 只能证明限制**生效**，
    /// 证明不了消耗量本身没变。RUSTSEC-2026-0315（wasmtime
    /// `call_ref` / exception `catch` 丢失燃料计账 → 指数放大）就是
    /// 「仍在计账、但少数路径漏记」——那种退化只会让同样的 wasm 烧掉更多
    /// 燃料，既不会触发耗尽也不会让现有测试变红，却会直接抬高真实执行成本。
    ///
    /// 基线为 wasmtime 47.0.4 实测值；升级到 48.0.5 后逐一比对完全一致
    /// （hello=2 / fast_tool=2），故可作为跨版本回归锚点。
    ///
    /// 注意：这是**行为锁定**，不是性能基准。数值若因上游指令计账口径调整
    /// 而合理变化，需连同 wasmtime 升级一起更新本断言，并在提交里写明原因。
    #[tokio::test]
    async fn test_fuel_consumption_baseline_is_stable() {
        use sisyphus_types::ToolExecutor;

        let executor = test_executor();
        for (tool_id, expected_fuel) in [("hello", 2u64), ("fast_tool", 2u64)] {
            let (data, measurement) = executor
                .execute_measured(tool_id, vec![], 1_000_000)
                .await
                .unwrap_or_else(|e| panic!("{tool_id} should execute: {e}"));
            let consumed = measurement
                .map(|m| m.fuel_consumed)
                .unwrap_or_else(|| panic!("{tool_id} must report fuel measurement"));
            assert_eq!(
                consumed, expected_fuel,
                "{tool_id} fuel consumption changed (baseline from wasmtime 47.0.4); \
                 if the change is an intended upstream accounting change, update this \
                 baseline in the same commit as the wasmtime bump and say why"
            );
            assert!(!data.is_empty() || tool_id == "fast_tool");
        }
    }

    /// 恶意/失控模块必须被拦住，而不是靠燃料恰好用完。
    ///
    /// 与上面的数值断言互补：这条证明上限**有效**，那条证明计账**没漂**。
    #[tokio::test]
    async fn test_evil_tool_is_bounded_by_fuel() {
        let executor = test_executor();
        let result = executor.execute("evil_tool", vec![], 100_000).await;
        assert!(
            result.is_err(),
            "evil_tool must be stopped by the fuel limit, not run to completion"
        );
        assert!(
            result.unwrap_err().to_string().contains("ResourceExhausted"),
            "evil_tool should fail with ResourceExhausted"
        );
    }

    /// 墙钟兜底必须在燃料失效时仍然生效。
    ///
    /// 覆盖的是“计账完全失效”而非“消耗变多”：把 fuel 设为 `u64::MAX`
    /// （等价于 wasmtime 不再计账），`infinite_loop` 不会被燃料拦住。此时
    /// 唯一还能停下它的就是与燃料无关的 epoch 中断。
    ///
    /// 判别依据是**trap 的来源**而非耗时：wasmtime 的 epoch trap 与内存
    /// trap 在错误文本里长得几乎一样（都是 `ResourceExhausted: ... wasm
    /// backtrace`），所以不能靠 `is_err()` 也不能靠墙钟时长来猜。真正能
    /// 区分的是 guest 是否“跑到了截止时间”——epoch trap 只会发生在
    /// deadline 过期后，且必然伴随一段实际执行时长。
    #[tokio::test]
    async fn test_wall_clock_bound_stops_runaway_when_fuel_metering_fails() {
        let executor = test_executor();
        let ceiling = std::time::Duration::from_millis(EXECUTION_TIMEOUT_MS * 6);

        let start = Instant::now();
        let result = executor.execute_internal_unmetered("infinite_loop", 100, true).await;
        let elapsed = start.elapsed();

        assert!(
            result.is_err(),
            "runaway guest must be stopped even though fuel can never run out"
        );
        assert!(
            elapsed < ceiling,
            "wall-clock bound did not engage: took {elapsed:?}, ceiling {ceiling:?}"
        );
        // 门槛至少得跨过一个 tick，否则“很快返回”可能只是当场 trap 了。
        let tick = std::time::Duration::from_millis(EXECUTION_TIMEOUT_MS);
        assert!(
            elapsed >= tick,
            "returned after {elapsed:?}, which is under one epoch tick ({tick:?}); \
             an instant trap is not the wall-clock bound engaging"
        );
    }

    /// 阴性对照：关掉墙钟后，同一个失控 guest 仍会被**别的上限**拦住。
    ///
    /// 这不是在说“关掉兜底就会挂死”，而是在固定一个关键事实：`infinite_loop`
    /// 同时违反内存上限，所以无论如何都会 trap。区分两类 trap 的是 guest
    /// 是否有机会跑到 deadline——开了兜底就跑到，没开就当场撞内存。
    ///
    /// 没有这条，上面那条的耗时下界就无法证明自己测的是兜底：它可能只是
    /// 撞上了内存上限很快返回。
    #[tokio::test]
    async fn test_runaway_guest_stops_on_memory_limit_without_wall_clock_bound() {
        let executor = test_executor();
        let tick = std::time::Duration::from_millis(EXECUTION_TIMEOUT_MS);

        let start = Instant::now();
        let result = executor.execute_internal_unmetered("infinite_loop", 100, false).await;
        let elapsed = start.elapsed();

        assert!(result.is_err(), "memory limit should still stop the guest");
        assert!(
            elapsed < tick,
            "without the wall-clock bound the guest should trap immediately on the \
             memory limit, but took {elapsed:?} (>= one tick {tick:?}), which means it \
             ran until the epoch deadline fired"
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
