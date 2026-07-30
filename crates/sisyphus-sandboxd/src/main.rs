//! sisyphus-sandboxd — standalone WASM sandbox daemon.
//!
//! Supports two transport modes:
//!
//! - **stdio** (default): JSON-line protocol over stdin/stdout.
//!   Request/response structure matches a simplified JSON-RPC shape.
//!
//! - **grpc**: typed gRPC service via tonic.
//!   Pass `--mode grpc --listen-addr 127.0.0.1:50052`.
//!
//! # stdio protocol
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
//!
//! # gRPC protocol
//!
//! See `proto/sandboxd.proto` for the schema.  The `SandboxdService` exposes
//! `ExecuteTool`, `Ping`, and `Shutdown` RPCs.  Raw bytes are passed directly
//! (no base64 overhead).

use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_types::ToolExecutor;
use std::path::PathBuf;
use std::sync::Arc;
use std::net::SocketAddr;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Notify;
use tracing::{error, info, warn};

// ── Generated gRPC bindings ─────────────────────────────────────────────────

pub mod pb {
    tonic::include_proto!("sisyphus.sandboxd");
}

// ── stdio protocol types ────────────────────────────────────────────────────

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

// ── CLI argument parsing ────────────────────────────────────────────────────

#[derive(Debug)]
struct Args {
    mode: Mode,
    listen_addr: SocketAddr,
    tools_dir: PathBuf,
    max_memory_pages: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Stdio,
    Grpc,
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stdio" => Ok(Mode::Stdio),
            "grpc" => Ok(Mode::Grpc),
            other => Err(format!("Unknown mode '{other}'; expected 'stdio' or 'grpc'")),
        }
    }
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut mode = Mode::Stdio;
        // Default listen addr: port 0 so the OS assigns an ephemeral port.
        let mut listen_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let mut tools_dir = PathBuf::from("tools");
        let mut max_memory_pages = None;

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--mode" => {
                    let val = args.next().ok_or("--mode requires a value")?;
                    mode = val.parse()?;
                }
                "--listen-addr" => {
                    let val = args.next().ok_or("--listen-addr requires a value")?;
                    listen_addr = val.parse().map_err(|e| {
                        format!("Invalid --listen-addr '{val}': {e}")
                    })?;
                }
                "--tools-dir" => {
                    tools_dir = PathBuf::from(args.next().ok_or("--tools-dir requires a value")?);
                }
                "--max-memory-pages" => {
                    let val: u32 = args
                        .next()
                        .ok_or("--max-memory-pages requires a value")?
                        .parse()
                        .map_err(|e| format!("Invalid --max-memory-pages: {e}"))?;
                    max_memory_pages = Some(val);
                }
                other => {
                    return Err(format!("Unknown argument: {other}"));
                }
            }
        }

        Ok(Args {
            mode,
            listen_addr,
            tools_dir,
            max_memory_pages,
        })
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = match Args::parse() {
        Ok(a) => a,
        Err(e) => {
            error!(error = %e, "Failed to parse arguments");
            eprintln!("Usage: sandboxd [--mode stdio|grpc] [--listen-addr IP:PORT] [--tools-dir DIR] [--max-memory-pages N]");
            std::process::exit(1);
        }
    };

    let executor = Arc::new(
        match WasmToolExecutor::with_config(Some(args.tools_dir.clone()), args.max_memory_pages) {
            Ok(exec) => exec,
            Err(e) => {
                error!(error = %e, "Failed to create executor");
                std::process::exit(1);
            }
        },
    );

    info!(
        mode = ?args.mode,
        tools_dir = %args.tools_dir.display(),
        max_memory_pages = ?args.max_memory_pages,
        "Starting sandbox daemon"
    );

    match args.mode {
        Mode::Stdio => {
            if let Err(e) = run_stdio(executor).await {
                error!(error = %e, "Stdio mode failed");
                std::process::exit(1);
            }
        }
        Mode::Grpc => {
            if let Err(e) = run_grpc(executor, args.listen_addr).await {
                error!(error = %e, "gRPC mode failed");
                std::process::exit(1);
            }
        }
    }
}

// ── Stdio mode ──────────────────────────────────────────────────────────────

/// Run the JSON-line stdio protocol loop.
async fn run_stdio(executor: Arc<WasmToolExecutor>) -> Result<(), Box<dyn std::error::Error>> {
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

    info!("Sandbox daemon (stdio) exiting");
    Ok(())
}

// ── gRPC mode ───────────────────────────────────────────────────────────────

/// Run the gRPC server until a Shutdown RPC is received.
async fn run_grpc(
    executor: Arc<WasmToolExecutor>,
    addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let shutdown = Arc::new(Notify::new());

    let service = pb::sandboxd_service_server::SandboxdServiceServer::new(
        SandboxdServiceImpl {
            executor,
            shutdown: shutdown.clone(),
        },
    );

    info!(listen_addr = %addr, "Starting gRPC server");

    tonic::transport::Server::builder()
        .add_service(service)
        .serve_with_shutdown(addr, async move {
            shutdown.notified().await;
            info!("Shutdown signal received, stopping gRPC server");
        })
        .await?;

    info!("Sandbox daemon (gRPC) exiting");
    Ok(())
}

/// gRPC service implementation backed by `WasmToolExecutor`.
struct SandboxdServiceImpl {
    executor: Arc<WasmToolExecutor>,
    /// Notified when a Shutdown RPC is received, triggering graceful shutdown.
    shutdown: Arc<Notify>,
}

#[tonic::async_trait]
impl pb::sandboxd_service_server::SandboxdService for SandboxdServiceImpl {
    async fn execute_tool(
        &self,
        request: tonic::Request<pb::ExecuteToolRequest>,
    ) -> Result<tonic::Response<pb::ExecuteToolResponse>, tonic::Status> {
        let req = request.into_inner();
        info!(
            tool_id = %req.tool_id,
            args_len = req.args.len(),
            fuel = req.fuel_limit,
            "gRPC execute_tool"
        );

        match self
            .executor
            .execute(&req.tool_id, req.args, req.fuel_limit)
            .await
        {
            Ok(data) => {
                let response = pb::ExecuteToolResponse {
                    data,
                    fuel_consumed: 0, // WasmToolExecutor does not report fuel consumed yet
                    error: String::new(),
                };
                Ok(tonic::Response::new(response))
            }
            Err(e) => {
                let response = pb::ExecuteToolResponse {
                    data: Vec::new(),
                    fuel_consumed: 0,
                    error: e.to_string(),
                };
                Ok(tonic::Response::new(response))
            }
        }
    }

    async fn ping(
        &self,
        _request: tonic::Request<pb::PingRequest>,
    ) -> Result<tonic::Response<pb::PingResponse>, tonic::Status> {
        Ok(tonic::Response::new(pb::PingResponse { pong: true }))
    }

    async fn shutdown(
        &self,
        _request: tonic::Request<pb::ShutdownRequest>,
    ) -> Result<tonic::Response<pb::ShutdownResponse>, tonic::Status> {
        info!("Received gRPC shutdown request");
        self.shutdown.notify_one();
        Ok(tonic::Response::new(pb::ShutdownResponse {}))
    }
}

// ── Base64 helpers (stdio mode only) ────────────────────────────────────────

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
