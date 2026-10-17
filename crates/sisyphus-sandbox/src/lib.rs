use async_trait::async_trait;
use dashmap::DashMap;
use sisyphus_metrics::record_wasm_execution;
use sisyphus_types::{SandboxErrorKind, ToolExecutor};
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
    /// The guest trapped for a reason the sandbox did not impose — an
    /// `unreachable`, an out-of-bounds access, a stack overflow.
    ///
    /// This used to be the catch-all for *every* trap and carried a
    /// `ResourceExhausted: ` prefix, which mislabelled guest bugs as resource
    /// limits. The prefix is gone because the message now means what it says.
    #[error("WASM execution failed: {0}")]
    ExecutionError(String),
    /// A resource limit the sandbox imposed actually stopped the guest.
    ///
    /// Split out from [`SandboxError::ExecutionError`] because the two mean
    /// opposite things to a caller: this one is the sandbox working, the other is
    /// a misbehaving module. They used to be indistinguishable, because every
    /// trap was wrapped in `ExecutionError`, whose `ResourceExhausted: ` prefix
    /// labelled *all* of them as resource exhaustion — including a guest
    /// `unreachable` and an out-of-bounds access, where the label simply lied.
    #[error("ResourceExhausted: {tool_id} stopped the guest ({kind}): {details}")]
    ResourceExhausted {
        tool_id: String,
        /// Which limit fired. This *is* the reason — `SandboxErrorKind`'s
        /// `Display` renders it — so there is no second, redundant field that
        /// could disagree with it.
        kind: SandboxErrorKind,
        details: String,
    },
}

/// Classify a guest trap into the category it belongs to.
///
/// wasmtime 48 exposes no trap classifier on `Error` (`trap_code()` is gone),
/// but `Error::downcast_ref::<Trap>()` still resolves the underlying trap, so
/// the distinction is recoverable without parsing message text.
///
/// Every non-resource trap maps to [`SandboxErrorKind::GuestTrap`] rather than
/// to a distinct kind per trap: from the caller's perspective `unreachable`,
/// out-of-bounds, and stack overflow are the same thing — the module is broken —
/// and the exact trap remains available in the rendered `details`.
///
/// The `_` arm is the load-bearing part: a trap wasmtime adds in a future
/// version is, by definition, not one of the limits this sandbox configures, so
/// it must not be mistaken for a resource limit.
fn classify_trap(trap: &Trap) -> SandboxErrorKind {
    match trap {
        Trap::OutOfFuel => SandboxErrorKind::FuelExhausted,
        Trap::Interrupt => SandboxErrorKind::WallClockExceeded,
        _ => SandboxErrorKind::GuestTrap,
    }
}

impl SandboxError {
    /// The wire category for this error.
    ///
    /// Every variant is mapped explicitly, including the ones that look alike.
    /// That is deliberate: the previous state of this code had a single
    /// catch-all, so a missing `.wasm`, a broken module, and an exhausted fuel
    /// budget all reached the caller as one opaque string. An exhaustive `match`
    /// makes the compiler flag a newly added variant instead of letting it
    /// silently inherit a wrong category.
    pub fn kind(&self) -> SandboxErrorKind {
        match self {
            // The engine itself could not be built — a sandbox problem, not a
            // guest problem.
            Self::EngineCreationFailed(_) => SandboxErrorKind::SandboxUnavailable,
            Self::IoError { .. }
            | Self::CompileError { .. }
            | Self::InstantiationError { .. }
            | Self::TaskJoinError(_) => SandboxErrorKind::ToolLoadFailed,
            // Setting the fuel budget failed: the limit could not even be armed.
            // Not the guest exhausting fuel — the sandbox failing to equip.
            Self::FuelError { .. } => SandboxErrorKind::SandboxUnavailable,
            Self::MissingExecuteExport(_) => SandboxErrorKind::MissingExport,
            // This one carries the category chosen by `classify_trap`, so it is already
            // decided by the time we get here.
            Self::ResourceExhausted { kind, .. } => *kind,
            Self::ExecutionError(_) => SandboxErrorKind::GuestTrap,
        }
    }

    /// The failing tool's id, when this error knows it.
    ///
    /// Used to populate [`ToolExecutionError::tool_id`] on the wire. Errors that
    /// precede compilation (engine creation) have no tool, which is why this is
    /// an `Option` rather than an empty-string convention.
    pub fn tool_id(&self) -> Option<&str> {
        match self {
            Self::IoError { tool_id, .. }
            | Self::CompileError { tool_id, .. }
            | Self::FuelError { tool_id, .. }
            | Self::InstantiationError { tool_id, .. }
            | Self::ResourceExhausted { tool_id, .. } => Some(tool_id),
            Self::MissingExecuteExport(tool_id) => Some(tool_id),
            Self::EngineCreationFailed(_) | Self::TaskJoinError(_) | Self::ExecutionError(_) => {
                None
            },
        }
    }
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

/// Result of one internal execution: payload, fuel spent, peak linear memory.
///
/// Named rather than spelled as a 3-tuple because the third element was easy
/// to drop silently at call sites (`let (data, _) = ...`), which is exactly how
/// the memory measurement gap appeared in the first place.
type ExecOutcome = (Vec<u8>, u64, Option<u64>);

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
    /// `None` only in the negative-control build (see [`with_config_inner`]).
    _epoch_ticker: Option<Arc<EpochTicker>>,
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
        Self::with_config_inner(tools_path, max_memory_pages, false)
    }

    /// Build a sandbox with the wall-clock backstop explicitly on or off.
    ///
    /// Not part of the supported surface: the only production caller is
    /// `sisyphus-sandboxd`, which needs `false` to run its negative-control
    /// test (`test_without_wall_clock_bound_a_runaway_guest_hangs`) through its
    /// real stdio protocol. Turning the backstop off is never a production
    /// choice, so this stays `#[doc(hidden)]` rather than joining
    /// [`with_config`](Self::with_config) — changing that signature would ripple
    /// through every caller for a knob nobody should reach for.
    ///
    /// # Safety of the knob
    ///
    /// `wall_clock_enabled = false` leaves fuel as the only limit. It is
    /// honoured at **config** level (no `epoch_interruption`, no ticker), not
    /// just by skipping `set_epoch_deadline`: with epoch interruption still on,
    /// a ticking engine traps *any* unarmed store immediately (measured ~30ms
    /// for `fast_tool`), which would prove nothing about a missing backstop.
    /// With it fully off, a runaway guest really does hang — which is exactly
    /// what the negative control asserts, and why it needs its own timeout.
    ///
    /// # Errors
    /// Same as [`with_config`](Self::with_config): engine creation failure.
    #[doc(hidden)]
    pub fn with_config_and_backstop(
        tools_path: Option<PathBuf>,
        max_memory_pages: Option<u32>,
        wall_clock_enabled: bool,
    ) -> Result<Self, SandboxError> {
        Self::with_config_inner(tools_path, max_memory_pages, !wall_clock_enabled)
    }

    /// Shared constructor; `wall_clock_disabled` exists solely for the negative
    /// control in `test_runaway_guest_hangs_without_any_backstop` and for
    /// [`with_config_and_backstop`](Self::with_config_and_backstop).
    ///
    /// It has to be honoured at **config** level, not only in
    /// `arm_epoch_deadline`: with `epoch_interruption` still enabled, a ticking
    /// engine traps *any* store whose deadline is unset. Measured, not assumed:
    /// skipping only the `set_epoch_deadline` call traps `infinite_loop` in
    /// ~20ms even though it exports no memory at all, so that shape cannot
    /// demonstrate "no backstop" — it just demonstrates a second trap source.
    fn with_config_inner(
        tools_path: Option<PathBuf>,
        max_memory_pages: Option<u32>,
        wall_clock_disabled: bool,
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
        if !wall_clock_disabled {
            config.epoch_interruption(true);
        }
        // wasmtime ≥46 defaults memory_reservation to 4GiB; the pooling allocator
        // pre-maps that much per slot, which hangs in memory-constrained envs.
        // Our WASM tools are small, so a modest reservation avoids the huge mmap.
        //
        // It must, however, be at least the pooling allocator's `max_memory_size`:
        // wasmtime rejects an engine whose reservation is smaller than the largest
        // memory a slot may hand out (`maximum memory size of 0x460000 bytes
        // exceeds the configured memory reservation of 0x200000 bytes`). A fixed
        // 2 MiB cap therefore made *every* `max_memory_pages` above 32 fail engine
        // construction outright — the sandbox would refuse to start rather than
        // enforce the limit. Floor the reservation at the configured budget so the
        // setting is always usable, and keep the 2 MiB floor for the default 10-page
        // budget, which is unchanged.
        const MIN_RESERVATION: u64 = 2 * 1024 * 1024;
        let max_memory_bytes = max_memory_pages as u64 * 65536;
        config.memory_reservation(MIN_RESERVATION.max(max_memory_bytes));

        let mut pooling = wasmtime::PoolingAllocationConfig::default();
        pooling
            .max_memory_size(max_memory_bytes as usize)
            // Conservative pool sizes to avoid large pre-allocated mmaps in memory-constrained envs
            .total_tables(10)
            .table_elements(100)
            .total_core_instances(10)
            .total_memories(10);
        config.allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling(pooling));

        let engine = Engine::new(&config)?;
        // Nothing to tick once epoch interruption is off; keeping the ticker
        // alive in that mode would advance a counter nobody reads.
        let epoch_ticker = if wall_clock_disabled {
            None
        } else {
            Some(Arc::new(EpochTicker::spawn(
                engine.clone(),
                std::time::Duration::from_millis(EXECUTION_TIMEOUT_MS),
            )))
        };

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
        args: &[u8],
        fuel_limit: u64,
    ) -> Result<ExecOutcome, sisyphus_types::TypesError> {
        self.execute_internal_opts(tool_id, args, fuel_limit, ExecOpts::DEFAULT).await
    }

    #[cfg(test)]
    pub(crate) async fn execute_internal_unmetered(
        &self,
        tool_id: &str,
        fuel_limit: u64,
        wall_clock: bool,
    ) -> Result<ExecOutcome, sisyphus_types::TypesError> {
        self.execute_internal_opts(
            tool_id,
            &[],
            fuel_limit,
            ExecOpts {
                metered: false,
                wall_clock,
            },
        )
        .await
    }

    /// Writes `args` into the guest's linear memory and returns its offset.
    ///
    /// Appended after the current data size so it cannot clobber the module's
    /// own data segments, and NUL-terminated to match the return convention.
    /// Returns 0 when the guest exports no `memory` or growth fails; the call
    /// still proceeds with a null pointer, which is how guests without memory
    /// behaved before.
    ///
    /// Note the input ABI differs from [`crate::custom_executor::CustomToolExecutor`]:
    /// this executor allocates guest-side (host appends, then passes the
    /// offset), while that one requires the guest to export
    /// `alloc(size: i32) -> i32`. A module written for one will not receive
    /// its arguments through the other.
    fn write_args_to_guest(
        &self,
        store: &mut wasmtime::Store<()>,
        instance: &wasmtime::Instance,
        args: &[u8],
    ) -> usize {
        if args.is_empty() {
            return 0;
        }
        let Some(memory) = instance.get_memory(&mut *store, "memory") else {
            return 0;
        };

        let offset = memory.data_size(&*store);
        let needed = offset + args.len() + 1;
        const WASM_PAGE: usize = 65_536;
        let have = (memory.size(&*store) as usize).saturating_mul(WASM_PAGE);
        if needed > have {
            let target_pages = needed.div_ceil(WASM_PAGE) as u64;
            let grow = target_pages.saturating_sub(memory.size(&*store));
            if grow > 0 && memory.grow(&mut *store, grow).is_err() {
                return 0;
            }
        }

        let mut buf = Vec::with_capacity(args.len() + 1);
        buf.extend_from_slice(args);
        buf.push(0);

        match memory.write(&mut *store, offset, &buf) {
            Ok(()) => offset,
            Err(_) => 0,
        }
    }

    async fn execute_internal_opts(
        &self,
        tool_id: &str,
        args: &[u8],
        fuel_limit: u64,
        opts: ExecOpts,
    ) -> Result<ExecOutcome, sisyphus_types::TypesError> {
        use sisyphus_types::TypesError;

        let span =
            tracing::info_span!("sandbox.execute", tool_id = %tool_id, fuel_limit = fuel_limit);
        let _enter = span.enter();

        let exec_start = Instant::now();
        let module = self.get_or_compile_module(tool_id).await.map_err(|e| {
            TypesError::sandbox(tool_id, SandboxErrorKind::ToolLoadFailed, e.to_string())
        })?;

        let linker = Linker::new(&self.engine);
        let mut store = Store::new(&self.engine, ());
        store.set_fuel(if opts.metered { fuel_limit } else { u64::MAX }).map_err(|e| {
            TypesError::sandbox(
                tool_id,
                SandboxErrorKind::SandboxUnavailable,
                SandboxError::FuelError {
                    tool_id: tool_id.to_string(),
                    source: e,
                }
                .to_string(),
            )
        })?;

        let instance = linker.instantiate(&mut store, &module).map_err(|e| {
            TypesError::sandbox(
                tool_id,
                SandboxErrorKind::ToolLoadFailed,
                SandboxError::InstantiationError {
                    tool_id: tool_id.to_string(),
                    source: e,
                }
                .to_string(),
            )
        })?;

        // Wall-clock backstop, independent of fuel. See the `epoch_interruption`
        // note in `with_config`: fuel can fail to stop a runaway loop, this cannot.
        self.arm_epoch_deadline(&mut store, opts.wall_clock);

        // 5. Look for the "execute" export with dynamic signature handling
        let execute_func = instance.get_func(&mut store, "execute").ok_or_else(|| {
            TypesError::sandbox(
                tool_id,
                SandboxErrorKind::MissingExport,
                SandboxError::MissingExecuteExport(tool_id.to_string()).to_string(),
            )
        })?;

        // Build argument list based on the function's expected parameter count.
        // ABI mirrors the return side: the guest returns an i32 pointer to a
        // NUL-terminated string in exported `memory`. Input uses the same
        // shape -- the host writes `args` into linear memory and passes the
        // pointer as the first parameter. Previously every parameter was a
        // hardcoded I32(0) and `args` was dropped outright, so WASM tools
        // always ran with empty input.
        let func_ty = execute_func.ty(&store);
        let params: Vec<Val> = match func_ty.params().len() {
            0 => vec![],
            n => {
                let ptr = self.write_args_to_guest(&mut store, &instance, args);
                let mut v = vec![Val::I32(ptr as i32); n];
                for slot in v.iter_mut().skip(1) {
                    *slot = Val::I32(0);
                }
                v
            },
        };

        let mut results = vec![Val::I32(0)];
        let call_start = Instant::now();
        let call_result = execute_func.call(&mut store, &params, &mut results).map_err(|e| {
            // Translate traps into a category a caller can act on. See `classify_trap`
            // for why the `ResourceExhausted` prefix used to be applied
            // unconditionally was wrong.
            let kind = e.downcast_ref::<Trap>().map_or(SandboxErrorKind::GuestTrap, classify_trap);
            let limit = kind.is_resource_limit();
            tracing::warn!(
                tool_id,
                kind = %kind,
                resource_limit = limit,
                error = %e,
                "WASM execution trapped"
            );
            let wire = if limit {
                SandboxError::ResourceExhausted {
                    tool_id: tool_id.to_string(),
                    kind,
                    details: e.to_string(),
                }
            } else {
                SandboxError::ExecutionError(e.to_string())
            };
            // `wire` already knows its category; keeping it typed lets the
            // daemon forward it instead of re-deriving it from this string.
            TypesError::sandbox(tool_id, wire.kind(), wire.to_string())
        });
        let call_duration = call_start.elapsed();

        // Check fuel consumption
        let fuel_consumed = fuel_limit.saturating_sub(store.get_fuel().unwrap_or(0));
        // Peak linear memory the guest actually committed to, as opposed to the
        // configured cap. `None` when the module exports no memory — an absent
        // measurement is not the same as zero bytes.
        let peak_memory =
            instance.get_memory(&mut store, "memory").map(|mem| mem.data_size(&store) as u64);
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
                    _ => return Ok((vec![], fuel_consumed, peak_memory)),
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

        Ok((result_bytes, fuel_consumed, peak_memory))
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
        let (data, _, _) = self.execute_internal(tool_id, &args, fuel_limit).await?;
        Ok(data)
    }

    async fn execute_measured(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<(Vec<u8>, Option<sisyphus_types::ExecutionMeasurement>), sisyphus_types::TypesError>
    {
        use sisyphus_types::TypesError;
        let exec_start = Instant::now();
        let (data, fuel_consumed, peak_memory_bytes) =
            self.execute_internal(tool_id, &args, fuel_limit).await.map_err(|e: TypesError| e)?;
        Ok((
            data,
            Some(sisyphus_types::ExecutionMeasurement {
                fuel_consumed,
                duration_ms: exec_start.elapsed().as_millis() as u64,
                peak_memory_bytes,
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

    /// 回归：args 必须真的抵达 guest。
    ///
    /// 此前 `execute` 直接丢弃 `args`，并且把所有参数硬编码成 `I32(0)`，
    /// 于是每个 WASM 工具都以空输入运行 —— 而资源限制相关测试只验证
    /// "能否停下来"，全都不受影响。`echo_args` guest 原样返回收到的指针，
    /// 宿主据此把参数读回来，因此本用例能观察到参数内容。
    #[tokio::test]
    async fn test_execute_passes_args_to_guest() {
        let executor = test_executor();
        let payload = br#"{"goal":"ship-it","n":42}"#;
        let result = executor.execute("echo_args", payload.to_vec(), 100_000).await;
        assert!(result.is_ok(), "execute failed: {:?}", result.err());
        let output = String::from_utf8(result.unwrap()).unwrap();
        assert_eq!(
            output, "{\"goal\":\"ship-it\",\"n\":42}",
            "guest must observe the exact args bytes"
        );
    }

    /// 空 args 必须保持既有行为（指针为 0，不写入内存）。
    #[tokio::test]
    async fn test_execute_with_empty_args_is_safe() {
        let executor = test_executor();
        let result = executor.execute("echo_args", vec![], 100_000).await;
        assert!(result.is_ok(), "execute failed: {:?}", result.err());
        assert!(result.unwrap().is_empty());
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
    /// 判别依据是**同进程内的相对对比**，而不是绝对耗时。
    ///
    /// 为什么不看错误文本：wasmtime 的 epoch trap 与内存 trap 在错误文本里几乎
    /// 一致（都是 `ResourceExhausted: ... wasm backtrace`），`is_err()` 只能证明
    /// 它停了，不能证明谁停的。
    ///
    /// 为什么不用绝对下界：`EpochTicker::spawn` 先 `sleep(interval)` 再
    /// `increment_epoch()`，而 store 的 deadline 相对引擎当前 epoch 值设定；
    /// 请求到达时刻与 ticker 相位无关，所以「等下一次 tick」的剩余时间在
    /// (0, interval] 之间浮动。实测（daemon 侧同构场景）14 轮里有 2 轮低于一个
    /// tick，因此 `>= interval` 是靠运气过的断言。换成固定 1s 能绕过抖动，却又把
    /// 「这台机器够不够快」写进了测试——慢机器或更大的工具可能轻松超过 1s。
    ///
    /// 现在改为同一个 executor 内先跑一次 `hello`（含编译与执行）量出
    /// `baseline`，再要求失控 guest 至少比它慢 10 倍。秒级的 epoch 等待 vs
    /// 毫秒级的正常执行，量级差异随机器速度一起缩放，任何机器上都成立。
    #[tokio::test]
    async fn test_wall_clock_bound_stops_runaway_when_fuel_metering_fails() {
        let executor = test_executor();
        let ceiling = std::time::Duration::from_millis(EXECUTION_TIMEOUT_MS * 6);

        // 同进程基线：一次正常执行。
        let warmup = Instant::now();
        let warmup_result = executor.execute_internal_unmetered("hello", 1_000, true).await;
        let baseline = warmup.elapsed();
        warmup_result.expect("baseline call must succeed");

        let floor = baseline.saturating_mul(10).max(std::time::Duration::from_millis(1));

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
        // 当场 trap（内存上限、未 arm 的 store）会与基线同量级；跑满一个 epoch
        // 周期则是秒级。相对判别比绝对阈值稳。
        assert!(
            elapsed >= floor,
            "returned after {elapsed:?}, not clearly slower than a normal execution \
             ({baseline:?}) — something else stopped the guest immediately"
        );
    }

    /// 回归：trap 的类别必须以类型形式抵达调用方。
    ///
    /// 此前 `SandboxError` 被 `to_string()` 拍平，daemon 只能写
    /// `kind_from_rendered_detail` 去匹配英文文案反推类别——而
    /// `ResourceExhausted` 与 guest `unreachable` 这两种相反的结果曾经共用
    /// 一个前缀。现在类别随错误一起传，文本匹配仅剩兜底。
    #[tokio::test]
    async fn trap_carries_its_kind_as_a_type() {
        let executor = test_executor();
        // Metered with a small budget so the fuel limit -- not the wall-clock
        // backstop -- is what stops the guest.
        let err = executor
            .execute("infinite_loop", vec![], 100)
            .await
            .expect_err("runaway guest must fail");

        let kind = err.sandbox_kind().expect("sandbox failure must carry a category");
        assert_eq!(
            kind,
            SandboxErrorKind::FuelExhausted,
            "fuel-limited guest must report FuelExhausted"
        );
    }

    /// 对照组：**只**关掉 store 上的 deadline，epoch 中断仍开启。
    ///
    /// 这条固定的是 wasmtime 的一个反直觉行为，也是写这条注释的原因：
    /// 只要 `epoch_interruption` 开着且引擎在被 tick，**任何**没设 deadline 的
    /// store 都会 trap——不需要 guest 真的跑到哪里。
    ///
    /// 之前这条测试的注释说「关掉兜底后 guest 会当场撞内存上限」。那是**错的**：
    /// `infinite_loop.wat` 根本没有 memory 导出，没有任何内存可供 grow。实测
    /// （本仓库，wasmtime 48）在只跳过 `set_epoch_deadline` 时它约 20ms 就 trap
    /// 了，而 epoch ticker 每 5s 才动一次——所以拦下它的绝不是内存上限。
    ///
    /// 那条错误归因有实际危害：它让人以为「关掉兜底只是换个上限拦」，于是不会
    /// 想到「关掉 epoch 后是真的会挂死」。后者由
    /// `test_runaway_guest_hangs_without_any_backstop` 真实测量。
    #[tokio::test]
    async fn test_unarmed_store_traps_immediately_while_epoch_is_still_enabled() {
        let executor = test_executor();

        // 无 memory 导出，所以内存上限这条路对它是封死的。
        let wat = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(|p| p.parent())
                .expect("workspace root")
                .join("tools")
                .join("infinite_loop.wat"),
        )
        .expect("read infinite_loop.wat");
        assert!(
            !wat.contains("export \"memory\""),
            "infinite_loop must stay memoryless for this test to mean anything"
        );

        let start = Instant::now();
        let result = executor.execute_internal_unmetered("infinite_loop", 100, false).await;
        let elapsed = start.elapsed();

        assert!(result.is_err(), "the guest must still be stopped");
        // 上界用「远小于一个 epoch 周期」而不是绝对 1s：上界比下界稳得多，
        // 而且这里要对比的正是「当场 trap」与「等满一个周期」。上界取半个周期，
        // 直接绑定到机制本身，而不是绑定到某台机器的速度。
        let immediate_ceiling = std::time::Duration::from_millis(EXECUTION_TIMEOUT_MS / 2);
        assert!(
            elapsed < immediate_ceiling,
            "expected an immediate trap from the unarmed store, but it took {elapsed:?} \
             (ceiling {immediate_ceiling:?}, less than half an epoch cycle)"
        );
    }

    /// 阴性对照：**完全**关掉墙钟兜底后，失控 guest 真的挂死。
    ///
    /// 这才是前一条所缺的对照。构造方式必须是在 `Config` 层关掉
    /// `epoch_interruption`，而不能只在 `arm_epoch_deadline` 里跳过——后者留下
    /// 一个仍在 tick 的引擎，它会当场 trap（见上一条），根本到不了「挂死」。
    ///
    /// `infinite_loop` 不导出 memory，燃料又已拉满（等价于计账失效），排除掉
    /// 内存与燃料之后，没有任何东西能停下它。
    ///
    /// 因为预期就是挂死，只能用「等不到返回」判定，所以自带超时——否则失败模式
    /// 是整个测试套件被 CI 杀掉，而不是一条清晰的断言失败。
    #[tokio::test]
    async fn test_runaway_guest_hangs_without_any_backstop() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let tools_path = manifest_dir.parent().unwrap().parent().unwrap().join("tools");
        let executor = WasmToolExecutor::with_config_inner(Some(tools_path), None, true)
            .expect("negative-control executor");
        let ceiling = std::time::Duration::from_secs(10);

        // 必须在**独立线程**上跑，不能用 `tokio::time::timeout` 包在本测试的
        // runtime 上：wasmtime 的 `call` 是同步阻塞的，会占死那个 worker 线程，
        // 于是超时定时器根本不会被调度。首次写成 async + timeout 的版本实测挂死
        // 10 分钟后被外部取消——挂死是真的，但超时机制形同虚设。
        //
        // 这里同步等待 channel：worker 线程永不返回，所以 `recv_timeout` 必然
        // 超时；反过来，如果它很快返回了（路径错、编译失败等），channel 会立刻
        // 送达，测试就会失败并把真实错误打出来。
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = futures_block_on(executor.execute_internal_unmetered(
                "infinite_loop",
                u64::MAX,
                true,
            ));
            let _ = tx.send(result);
        });

        match rx.recv_timeout(ceiling) {
            Ok(result) => panic!(
                "expected the guest to hang forever, but it returned: {:?}",
                result.err()
            ),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {},
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                panic!("worker thread panicked instead of hanging")
            },
        }
    }

    /// Minimal blocking executor for a future that is known never to complete.
    fn futures_block_on<F: std::future::Future>(fut: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime for the hanging worker")
            .block_on(fut)
    }

    /// 内存计量必须真的有值，不能永远是 `None`。
    ///
    /// 字段是在“跨版本内存对照无数据可测”之后补的，所以它本身需要一个
    /// 判别信号：若某天 `get_memory` 签名变化或导出名拼错，字段会静默退化为
    /// `None`，而所有其他测试照样绿——这正是它当初缺失的方式。
    ///
    /// 期望值 65536 = 64KiB = wasm 线性内存的最小一页。工具都不申请更多，
    /// 所以这是可复现的确定值；若上游默认值改变，需连同变更一起更新。
    #[tokio::test]
    async fn test_peak_memory_is_actually_measured() {
        use sisyphus_types::ToolExecutor;

        let executor = test_executor();
        for tool_id in ["hello", "fast_tool"] {
            let (_, measurement) = executor
                .execute_measured(tool_id, vec![], 1_000_000)
                .await
                .unwrap_or_else(|e| panic!("{tool_id} should execute: {e}"));
            let peak = measurement.and_then(|m| m.peak_memory_bytes).unwrap_or_else(|| {
                panic!("{tool_id}: peak_memory_bytes is None — memory is not being measured")
            });
            assert_eq!(
                peak, 65_536,
                "{tool_id} should report exactly one 64KiB wasm page"
            );
        }
    }

    /// The memory limit must actually bound a guest that grows its linear memory.
    ///
    /// This replaces a test that asserted almost nothing. The old
    /// `test_execute_memory_exhaustion` checked `result.is_err()` and then
    /// accepted any of `ResourceExhausted` / `memory` / `translation`. Since a
    /// *compile* failure is also an error — and `tools/memory_hog.wat` was
    /// written as `(memory.grow 1000)`, which wasmtime reads as a memory index
    /// and rejects with `unknown memory 1000` — the fixture never compiled and
    /// the test passed for a reason that had nothing to do with memory limits.
    ///
    /// Two facts drive the design here:
    ///
    /// 1. `memory.grow` **does not trap** on failure; it returns the previous
    ///    size, or `-1` when the grow is refused. So the observable is the
    ///    grow's return value, not an error. Each fixture renders that value as
    ///    a decimal string in linear memory and returns its address, which is
    ///    the ABI this executor speaks: the `i32` result of `execute` is read
    ///    as a pointer and the bytes there are the tool's output.
    /// 2. A single test cannot distinguish "the limit works" from "nothing can
    ///    grow at all". Both fixtures declare `min = 1` and no maximum of their
    ///    own, so the only thing separating them is the configured
    ///    `max_memory_pages`.
    ///
    /// The limit is exercised as configured, 10 pages: 1 + 4 = 5 fits,
    /// 1 + 64 = 65 does not.
    #[tokio::test]
    async fn test_memory_limit_bounds_a_growing_guest() {
        use sisyphus_types::ToolExecutor;

        let executor = test_executor();
        let output = |data: Vec<u8>| {
            String::from_utf8(data.clone())
                .unwrap_or_else(|e| panic!("grow fixture must emit utf-8, got {data:?}: {e}"))
        };

        // Positive control: growing by 4 pages stays inside the 10-page budget,
        // so `memory.grow` reports the previous size (1), not -1.
        let within = executor
            .execute("grow_within_limit", vec![], 1_000_000)
            .await
            .unwrap_or_else(|e| panic!("grow_within_limit must compile and run: {e}"));
        assert_eq!(
            output(within),
            "1",
            "a grow that fits in the page budget must report the previous size"
        );

        // Negative case: growing by 64 pages must be refused. Same executor,
        // same engine, same module shape — only the requested size differs.
        let past = executor
            .execute("grow_past_limit", vec![], 1_000_000)
            .await
            .unwrap_or_else(|e| panic!("grow_past_limit must compile and run: {e}"));
        assert_eq!(
            output(past),
            "-1",
            "a grow past max_memory_pages must return the -1 failure sentinel"
        );

        // `memory_hog` grows by 1000 pages, so it must fail the same way.
        let hog = executor
            .execute("memory_hog", vec![], 1_000_000)
            .await
            .unwrap_or_else(|e| panic!("memory_hog must compile and run: {e}"));
        assert_eq!(
            output(hog),
            "-1",
            "memory_hog must be refused by the page budget, not fail to compile"
        );
    }

    /// `max_memory_pages` must change the outcome, not merely sit in the config.
    ///
    /// The companion to `test_memory_limit_bounds_a_growing_guest`: that test
    /// fixes the limit at its default and proves the boundary sits where the
    /// documentation says. This one moves the limit and shows the boundary moves
    /// with it, which is what rules out a hard-coded 10 pages in the engine.
    #[tokio::test]
    async fn test_max_memory_pages_actually_moves_the_boundary() {
        use sisyphus_types::ToolExecutor;

        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools")
            .canonicalize()
            .expect("tools dir");
        let output = |data: Vec<u8>| String::from_utf8_lossy(&data).into_owned();

        // 1 + 64 = 65 pages needed. A 70-page budget must cover it, even though
        // the default 10-page budget refuses the very same module.
        let roomy = WasmToolExecutor::with_config(Some(dir.clone()), Some(70))
            .expect("engine with a 70-page budget");
        let roomy = roomy
            .execute("grow_past_limit", vec![], 1_000_000)
            .await
            .unwrap_or_else(|e| panic!("must compile and run under a 70-page budget: {e}"));
        assert_eq!(
            output(roomy),
            "1",
            "a 70-page budget must allow the 65-page grow that the default refuses"
        );

        // 1 + 4 = 5 pages needed. A 3-page budget must refuse it, even though
        // the default 10-page budget allows the very same module.
        let tight = WasmToolExecutor::with_config(Some(dir.clone()), Some(3))
            .expect("engine with a 3-page budget");
        let tight = tight
            .execute("grow_within_limit", vec![], 1_000_000)
            .await
            .unwrap_or_else(|e| panic!("must compile and run under a 3-page budget: {e}"));
        assert_eq!(
            output(tight),
            "-1",
            "a 3-page budget must refuse the 5-page grow that the default allows"
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
