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
//! {"id": 2, "result": {"data": "base64_encoded_result", "fuel_consumed": 12345, "peak_memory_bytes": 65536}}
//! {"id": 2, "error": "error message"}
//! ```

use clap::Parser;
use sisyphus_sandbox::{SandboxError, WasmToolExecutor};
use sisyphus_types::{ToolExecutor, TypesError};
use std::path::PathBuf;
use tokio::io::{stdin, stdout, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tracing::{error, info, warn};

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

/// True when the wall-clock backstop is deliberately disabled for the negative
/// control in `test_without_wall_clock_bound_a_runaway_guest_hangs`.
///
/// Requires the value to be exactly `1`: `std::env::var_os` treats a present
/// but empty variable as *set*, so `SISYPHUS_SANDBOXD_NO_WALL_CLOCK=` (a very easy
/// thing to write in a shell or CI config) would silently disable the backstop
/// in a production process. The backstop is a safety limit — an accidental
/// env-var typo must not turn it off.
fn wall_clock_backstop_disabled() -> bool {
    std::env::var("SISYPHUS_SANDBOXD_NO_WALL_CLOCK").as_deref() == Ok("1")
}

/// Build the executor this daemon serves requests from.
///
/// WASM execution itself lives in `sisyphus_sandbox::WasmToolExecutor`. This
/// daemon used to carry a copy-pasted copy of that entire path — engine config,
/// epoch ticker, module cache, `execute_internal` — and the duplication was not
/// cosmetic: it is exactly why the wall-clock backstop could go missing here
/// while the library kept it. The daemon now holds the single implementation
/// and owns only the protocol wrapped around it.
///
/// The one knob forwarded is the negative-control switch. The library honours it
/// at wasmtime `Config` level (no `epoch_interruption`, no ticker), not merely by
/// skipping `set_epoch_deadline`: with epoch interruption still enabled, a
/// ticking engine traps *any* unarmed store within milliseconds, which would
/// prove nothing about a missing backstop.
fn build_executor(args: &Args) -> Result<WasmToolExecutor, SandboxError> {
    WasmToolExecutor::with_config_and_backstop(
        Some(args.tools_dir.clone()),
        Some(args.max_memory_pages),
        !wall_clock_backstop_disabled(),
    )
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
    /// `Notification` message variant in the daemon protocol.
    ///
    /// 保留理由（ADR-007 反模式要求 `#[allow(dead_code)]` 必须给出正当理由）：
    /// JSON-RPC 规范要求服务端可主动推送 notification。本枚举是 daemon 对
    /// 客户端的**响应**类型，`Notification` 变体预留给未来的 server-push
    /// 通道（如 long-running execute 的进度回报）。
    ///
    /// **不可删除**：本枚举是 `#[serde(untagged)]`，变体集合参与 wire format。
    /// 删除后再加回来需要重新确认序列化顺序与 untagged 匹配优先级，属于
    /// 对外协议的破坏性变更。当前无任何构造该变体的代码，故必须显式豁免 lint。
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
    /// guest 线性内存实际占用字节数。与 `fuel_consumed` 同样向后兼容：
    /// 新增字段，旧客户端按 serde 默认行为忽略。
    #[serde(skip_serializing_if = "Option::is_none")]
    peak_memory_bytes: Option<u64>,
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
            stdout.flush().await.map_err(|e| TypesError::ToolExecutionError {
                tool_id: "ping".into(),
                details: e.to_string(),
            })?;
        },
        Request::Execute { id, params } => {
            let result =
                executor.execute_measured(&params.tool_id, params.args, params.fuel_limit).await;
            let resp = match result {
                Ok((data, measurement)) => Response::ExecuteSuccess {
                    id,
                    result: ExecuteResult {
                        data: base64_encode(&data),
                        fuel_consumed: measurement.map(|m| m.fuel_consumed),
                        peak_memory_bytes: measurement.and_then(|m| m.peak_memory_bytes),
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
            stdout.flush().await.map_err(|e| TypesError::ToolExecutionError {
                tool_id: "execute".into(),
                details: e.to_string(),
            })?;
        },
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

    let executor = build_executor(&args)?;

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
            },
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
            },
        };

        if let Err(e) = handle_request(&executor, request, &mut writer).await {
            error!(error = %e, "Failed to handle request");
        }
    }

    info!("sisyphus-sandboxd stopped");
    Ok(())
}
