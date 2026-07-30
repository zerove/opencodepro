//! sisyphus-sandboxd — standalone WASM sandbox daemon.
//!
//! Runs `WasmToolExecutor` in an isolated process, communicating with
//! the bridge via line-delimited JSON over stdio. This eliminates wasmtime
//! from the bridge's dependency tree (both compile-time and runtime),
//! solving the CI OOM problem at the architectural level.
//!
//! # Protocol
//!
//! Each line is a complete JSON message terminated by `\n`.
//!
//! **Request** (has `id`):
//! ```json
//! {"id": 1, "method": "execute", "params": {"tool_id": "hello", "args": [], "fuel_limit": 1000}}
//! ```
//!
//! **Response** (`id` matches request):
//! ```json
//! {"id": 1, "result": {"data": "SGV", "fuel_consumed": 42}}
//! {"id": 1, "error": "fuel exhausted"}
//! ```
//!
//! **Notification** (no `id`, no response expected):
//! ```json
//! {"method": "shutdown"}
//! ```

use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_types::ToolExecutor;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{error, info, warn};

#[derive(serde::Deserialize)]
struct Request {
    #[serde(default)]
    id: Option<u64>,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(serde::Serialize)]
struct Response {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Response {
    fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: u64, message: String) -> Self {
        Self {
            id,
            result: None,
            error: Some(message),
        }
    }
}

#[derive(serde::Deserialize)]
struct ExecuteParams {
    tool_id: String,
    #[serde(default)]
    args: Vec<u8>,
    #[serde(default)]
    fuel_limit: u64,
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Parse CLI arguments: `sandboxd [--tools-dir DIR] [--max-memory-pages N]`
    let mut tools_dir = PathBuf::from("tools");
    let mut max_memory_pages = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tools-dir" => {
                tools_dir = PathBuf::from(args.next().ok_or("--tools-dir requires a value")?);
            }
            "--max-memory-pages" => {
                let val: u32 = args
                    .next()
                    .ok_or("--max-memory-pages requires a value")?
                    .parse()?;
                max_memory_pages = Some(val);
            }
            other => {
                return Err(format!("Unknown argument: {other}").into());
            }
        }
    }

    info!(
        tools_dir = %tools_dir.display(),
        max_memory_pages = ?max_memory_pages,
        "Starting sandbox daemon"
    );

    let executor = Arc::new(
        WasmToolExecutor::with_config(Some(tools_dir), max_memory_pages)
            .map_err(|e| format!("Failed to create executor: {e}"))?,
    );

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                error!(error = %e, "Failed to parse request: {line}");
                continue;
            }
        };

        // Handle shutdown notification (no id = notification)
        if req.id.is_none() && req.method == "shutdown" {
            info!("Received shutdown notification, exiting");
            break;
        }

        let id = match req.id {
            Some(id) => id,
            None => {
                warn!("Ignoring notification with unknown method: {}", req.method);
                continue;
            }
        };

        let response = match req.method.as_str() {
            "execute" => {
                let params: ExecuteParams = match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        error!(error = %e, "Invalid execute params");
                        continue;
                    }
                };

                info!(tool_id = %params.tool_id, fuel = params.fuel_limit, "Executing tool");

                match executor
                    .execute(&params.tool_id, params.args, params.fuel_limit)
                    .await
                {
                    Ok(data) => {
                        let result = serde_json::json!({
                            "data": base64_encode(&data),
                        });
                        Response::ok(id, result)
                    }
                    Err(e) => Response::err(id, e.to_string()),
                }
            }
            "ping" => Response::ok(id, serde_json::json!({"pong": true})),
            other => Response::err(id, format!("Unknown method: {other}")),
        };

        let json = serde_json::to_string(&response)?;
        stdout.write_all(json.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    info!("Sandbox daemon exiting");
    Ok(())
}

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    // Simple base64 encoding without external dependency
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    if let Err(e) = run().await {
        error!(error = %e, "Sandbox daemon failed");
        std::process::exit(1);
    }
}
