//! `sisyphus-sandboxd` — Standalone WASM sandbox daemon.
//!
//! Runs wasmtime in an isolated process, communicating with the bridge via
//! line-delimited JSON over stdio. This removes heavy wasmtime/LLVM dependencies
//! from the bridge process, solving CI OOM issues.
//!
//! # Protocol (v1 — stdio JSON-line)
//!
//! Each line is a complete JSON message terminated by `\n`.
//!
//! ## Requests (have `id` field; responses echo the `id`)
//!
//! ```json
//! {"id": 1, "method": "ping", "params": {}}
//! {"id": 2, "method": "execute", "params": {"tool_id": "hello", "args": [], "fuel_limit": 1000000}}
//! ```
//!
//! ## Notifications (no `id`)
//!
//! ```json
//! {"method": "shutdown"}
//! ```
//!
//! ## Responses
//!
//! ```json
//! {"id": 1, "result": {"pong": true}}
//! {"id": 2, "result": {"data": "base64_encoded_result", "fuel_consumed": 12345}}
//! {"id": 2, "error": "error message"}
//! ```

use async_trait::async_trait;
use clap::Parser;
use dashmap::DashMap;
use sisyphus_metrics::record_wasm_execution;
use sisyphus_types::{CacheInvalidatable, ToolExecutor, TypesError};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::io::{stdin, stdout, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tracing::{debug, error, info, warn};
use wasmtime::*;

#[derive(Parser, Debug)]
#[command(name = "sisyphus-sandboxd", version, about = "WASM sandbox daemon")]
struct Args {
    /// Directory containing .wasm tool binaries
    #[arg(long, default_value = "tools")]
    tools_dir: PathBuf,

    /// WASM linear memory limit in 64 KiB pages
    #[arg(long, default_value = "10")]
    max_memory_pages: u32,
}

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
struct WasmToolExecutor {
    engine: Engine,
    tools_path: PathBuf,
    module_cache: Arc<DashMap<String, Module>>,
}

impl CacheInvalidatable for WasmToolExecutor {
    fn invalidate_cache(&self, tool_id: &str) {
        debug!(tool_id = %tool_id, "Invalidating WASM module cache");
        self.module_cache.remove(tool_id);
    }
}

impl WasmToolExecutor {
    fn new(tools_path: PathBuf, max_memory_pages: u32) -> Result<Self, SandboxError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.max_wasm_stack(1_000_000);
        // wasmtime ≥46 defaults memory_reservation to 4GiB; the pooling allocator
        // pre-maps that much per slot, which hangs in memory-constrained envs.
        // Our WASM tools are small, so a modest reservation avoids the huge mmap.
        config.memory_reservation(2 * 1024 * 1024);

        let mut pooling = wasmtime::PoolingAllocationConfig::default();
        pooling
            .max_memory_size(max_memory_pages as usize * 65536)
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
            debug!("Module cache hit");
            return Ok(cached.clone());
        }

        let start = Instant::now();
        let wasm_bytes = self.load_wasm_bytes(tool_id).await?;
        let load_duration = start.elapsed();
        debug!(
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
        info!(tool_id = %tool_id, duration_ms = compile_duration.as_millis(), "WASM module compiled");

        self.module_cache
            .insert(tool_id.to_string(), module.clone());
        debug!("Module cached");

        Ok(module)
    }
}

impl WasmToolExecutor {
    /// 带测量的 WASM 执行（P2-9 燃料真值打通的核心实现）。
    async fn execute_internal(
        &self,
        tool_id: &str,
        fuel_limit: u64,
    ) -> Result<(Vec<u8>, u64), TypesError> {
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

        let execute_func = instance.get_func(&mut store, "execute").ok_or_else(|| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: SandboxError::MissingExecuteExport(tool_id.to_string()).to_string(),
            }
        })?;

        let func_ty = execute_func.ty(&store);
        let params: Vec<Val> = match func_ty.params().len() {
            0 => vec![],
            n => vec![Val::I32(0); n],
        };

        let mut results = vec![Val::I32(0)];
        let call_start = Instant::now();
        let call_result = execute_func
            .call(&mut store, &params, &mut results)
            .map_err(|e| TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: SandboxError::ExecutionError(e.to_string()).to_string(),
            });
        let call_duration = call_start.elapsed();

        let fuel_consumed = fuel_limit.saturating_sub(store.get_fuel().unwrap_or(0));
        debug!(
            fuel_consumed,
            call_duration_ms = call_duration.as_millis(),
            "WASM function call completed"
        );

        let result_bytes = match call_result {
            Ok(_) => {
                let result_ptr = match results.first() {
                    Some(Val::I32(ptr)) => *ptr as usize,
                    _ => return Ok((vec![], fuel_consumed)),
                };

                if let Some(memory) = instance.get_memory(&mut store, "memory") {
                    let data = memory.data(&store);
                    if result_ptr < data.len() {
                        let end = data[result_ptr..]
                            .iter()
                            .position(|&b| b == 0)
                            .map(|pos| result_ptr + pos)
                            .unwrap_or(data.len());
                        data[result_ptr..end].to_vec()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            }
            Err(e) => {
                warn!(error = %e, fuel_consumed, "WASM execution failed");
                return Err(e);
            }
        };

        let exec_duration = exec_start.elapsed();
        info!(tool_id = %tool_id, duration_ms = exec_duration.as_millis(), fuel_consumed, result_bytes = result_bytes.len(), "WASM execution completed");

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
    ) -> Result<Vec<u8>, TypesError> {
        let _ = args;
        let (data, _) = self.execute_internal(tool_id, fuel_limit).await?;
        Ok(data)
    }

    async fn execute_measured(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<(Vec<u8>, Option<sisyphus_types::ExecutionMeasurement>), TypesError> {
        let exec_start = std::time::Instant::now();
        let (data, fuel_consumed) = self.execute_internal(tool_id, fuel_limit).await?;
        Ok((
            data,
            Some(sisyphus_types::ExecutionMeasurement {
                fuel_consumed,
                duration_ms: exec_start.elapsed().as_millis() as u64,
            }),
        ))
    }

    fn as_cache_invalidatable(&self) -> Option<&dyn CacheInvalidatable> {
        Some(self)
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(tag = "method", rename_all = "lowercase")]
enum Request {
    Ping { id: u64 },
    Execute { id: u64, params: ExecuteParams },
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ExecuteParams {
    tool_id: String,
    args: Vec<u8>,
    fuel_limit: u64,
}

#[derive(serde::Serialize)]
#[serde(untagged)]
enum Response {
    Ping {
        id: u64,
        result: PingResult,
    },
    ExecuteSuccess {
        id: u64,
        result: ExecuteResult,
    },
    ExecuteError {
        id: u64,
        error: String,
    },
    #[allow(dead_code)]
    Notification {
        method: String,
    },
}

#[derive(serde::Serialize)]
struct PingResult {
    pong: bool,
}

#[derive(serde::Serialize)]
struct ExecuteResult {
    data: String, // base64 encoded
    /// 实际燃料消耗（P2-9）。旧客户端忽略此字段（向后兼容）。
    #[serde(skip_serializing_if = "Option::is_none")]
    fuel_consumed: Option<u64>,
}

fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len() * 4 / 3 + 4);
    let mut i = 0;
    while i + 2 < input.len() {
        let b1 = input[i];
        let b2 = input[i + 1];
        let b3 = input[i + 2];
        output.push(CHARS[(b1 >> 2) as usize] as char);
        output.push(CHARS[((b1 & 0x3) << 4 | b2 >> 4) as usize] as char);
        output.push(CHARS[((b2 & 0xf) << 2 | b3 >> 6) as usize] as char);
        output.push(CHARS[(b3 & 0x3f) as usize] as char);
        i += 3;
    }
    let remaining = input.len() - i;
    if remaining == 1 {
        let b1 = input[i];
        output.push(CHARS[(b1 >> 2) as usize] as char);
        output.push(CHARS[((b1 & 0x3) << 4) as usize] as char);
        output.push('=');
        output.push('=');
    } else if remaining == 2 {
        let b1 = input[i];
        let b2 = input[i + 1];
        output.push(CHARS[(b1 >> 2) as usize] as char);
        output.push(CHARS[((b1 & 0x3) << 4 | b2 >> 4) as usize] as char);
        output.push(CHARS[((b2 & 0xf) << 2) as usize] as char);
        output.push('=');
    }
    output
}

async fn handle_request(
    executor: &WasmToolExecutor,
    request: Request,
    stdout: &mut BufWriter<tokio::io::Stdout>,
) -> Result<(), TypesError> {
    match request {
        Request::Ping { id } => {
            let resp = Response::Ping {
                id,
                result: PingResult { pong: true },
            };
            let line =
                serde_json::to_string(&resp).map_err(|e| TypesError::ToolExecutionError {
                    tool_id: "ping".into(),
                    details: format!("Failed to serialize response: {e}"),
                })? + "\n";
            stdout.write_all(line.as_bytes()).await.map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: "ping".into(),
                    details: e.to_string(),
                }
            })?;
            stdout
                .flush()
                .await
                .map_err(|e| TypesError::ToolExecutionError {
                    tool_id: "ping".into(),
                    details: e.to_string(),
                })?;
        }
        Request::Execute { id, params } => {
            let result = executor
                .execute_measured(&params.tool_id, params.args, params.fuel_limit)
                .await;
            let resp = match result {
                Ok((data, measurement)) => Response::ExecuteSuccess {
                    id,
                    result: ExecuteResult {
                        data: base64_encode(&data),
                        fuel_consumed: measurement.map(|m| m.fuel_consumed),
                    },
                },
                Err(e) => Response::ExecuteError {
                    id,
                    error: e.to_string(),
                },
            };
            let line =
                serde_json::to_string(&resp).map_err(|e| TypesError::ToolExecutionError {
                    tool_id: "execute".into(),
                    details: format!("Failed to serialize response: {e}"),
                })? + "\n";
            stdout.write_all(line.as_bytes()).await.map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: "execute".into(),
                    details: e.to_string(),
                }
            })?;
            stdout
                .flush()
                .await
                .map_err(|e| TypesError::ToolExecutionError {
                    tool_id: "execute".into(),
                    details: e.to_string(),
                })?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    info!(
        tools_dir = %args.tools_dir.display(),
        max_memory_pages = args.max_memory_pages,
        "Starting sisyphus-sandboxd"
    );

    let executor = WasmToolExecutor::new(args.tools_dir, args.max_memory_pages)?;

    let mut reader = BufReader::new(stdin());
    let mut writer = BufWriter::new(stdout());
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            // EOF - parent closed stdin, shut down gracefully
            info!("Received EOF, shutting down");
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse as Request (with id) or Notification (no id)
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "Failed to parse JSON request");
                continue;
            }
        };

        // Check if it's a notification (no id field)
        if value.get("id").is_none() {
            if let Some(method) = value.get("method").and_then(|v| v.as_str()) {
                if method == "shutdown" {
                    info!("Received shutdown notification");
                    break;
                }
                warn!(method = %method, "Unknown notification");
            }
            continue;
        }

        // It's a request with id
        let request: Request = match serde_json::from_value(value.clone()) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Failed to deserialize request");
                // Respond with an error so the client never hangs waiting for a
                // response to an unknown method or malformed request.
                let id = value.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                let resp = Response::ExecuteError {
                    id,
                    error: format!("Unknown method or malformed request: {e}"),
                };
                if let Ok(line) = serde_json::to_string(&resp) {
                    let _ = writer.write_all((line + "\n").as_bytes()).await;
                    let _ = writer.flush().await;
                }
                continue;
            }
        };

        if let Err(e) = handle_request(&executor, request, &mut writer).await {
            error!(error = %e, "Failed to handle request");
        }
    }

    info!("sisyphus-sandboxd stopped");
    Ok(())
}
