//! Client for communicating with `sisyphus-sandboxd` over JSON-over-stdio IPC.
//!
//! Spawns the sandbox daemon as a child process and forwards `ToolExecutor`
//! calls via line-delimited JSON requests/responses. This eliminates wasmtime
//! from the bridge's address space.
//!
//! # Protocol
//!
//! See `crates/sisyphus-sandboxd/src/main.rs` for the full protocol spec.

use async_trait::async_trait;
use sisyphus_types::{CacheInvalidatable, ToolExecutor, ToolMetadata, TypesError};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tracing::{info, warn};

struct SandboxdConnection {
    child: Child,
    stdin: tokio::io::BufWriter<tokio::process::ChildStdin>,
    stdout: BufReader<tokio::process::ChildStdout>,
}

/// A `ToolExecutor` that delegates to an external `sisyphus-sandboxd` process
/// via JSON-over-stdio IPC.
pub struct SandboxdClient {
    conn: Arc<tokio::sync::Mutex<SandboxdConnection>>,
    next_id: AtomicU64,
}

impl SandboxdClient {
    /// Spawn the `sisyphus-sandboxd` child process and return a client handle.
    ///
    /// `sandboxd_path` — path to the binary, or `"sisyphus-sandboxd"` to search PATH.
    /// `tools_dir` — directory containing `.wasm` tool binaries.
    /// `max_memory_pages` — WASM linear memory limit (64 KiB pages).
    pub async fn spawn(
        sandboxd_path: &str,
        tools_dir: &str,
        max_memory_pages: u32,
    ) -> Result<Self, TypesError> {
        let mut child = Command::new(sandboxd_path)
            .arg("--tools-dir")
            .arg(tools_dir)
            .arg("--max-memory-pages")
            .arg(max_memory_pages.to_string())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| TypesError::ToolExecutionError {
                tool_id: "spawn".into(),
                details: format!("Failed to spawn sandboxd: {e}"),
            })?;

        let stdin = child.stdin.take().ok_or_else(|| TypesError::ToolExecutionError {
            tool_id: "spawn".into(),
            details: "Failed to capture sandboxd stdin".into(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| TypesError::ToolExecutionError {
            tool_id: "spawn".into(),
            details: "Failed to capture sandboxd stdout".into(),
        })?;

        let conn = SandboxdConnection {
            child,
            stdin: tokio::io::BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
        };

        let client = Self {
            conn: Arc::new(tokio::sync::Mutex::new(conn)),
            next_id: AtomicU64::new(1),
        };

        // Verify the daemon is alive with a ping
        match client.ping().await {
            Ok(()) => info!("Sandbox daemon is alive"),
            Err(e) => warn!(error = %e, "Sandbox daemon ping failed (may start slower)"),
        }

        Ok(client)
    }

    /// Send a ping request to verify the daemon is responsive.
    pub async fn ping(&self) -> Result<(), TypesError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = serde_json::json!({
            "id": id,
            "method": "ping",
            "params": {},
        });

        let response = self.send_request(id, &request).await?;

        if response.get("result").and_then(|r| r.get("pong")) != Some(&serde_json::json!(true)) {
            return Err(TypesError::ToolExecutionError {
                tool_id: "ping".into(),
                details: "Unexpected ping response".into(),
            });
        }
        Ok(())
    }

    /// Send a request and wait for a matching response.
    async fn send_request(
        &self,
        id: u64,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, TypesError> {
        let mut conn = self.conn.lock().await;

        // Write request line
        let line = serde_json::to_string(request).map_err(|e| TypesError::ToolExecutionError {
            tool_id: "serialize".into(),
            details: format!("Failed to serialize request: {e}"),
        })?;
        conn.stdin.write_all(line.as_bytes()).await.map_err(|e| {
            TypesError::ToolExecutionError {
                tool_id: "write".into(),
                details: format!("Failed to write to sandboxd: {e}"),
            }
        })?;
        conn.stdin.write_all(b"\n").await.map_err(|e| TypesError::ToolExecutionError {
            tool_id: "write".into(),
            details: format!("Failed to write newline to sandboxd: {e}"),
        })?;
        conn.stdin.flush().await.map_err(|e| TypesError::ToolExecutionError {
            tool_id: "flush".into(),
            details: format!("Failed to flush sandboxd stdin: {e}"),
        })?;

        // Read lines until we find the matching response id
        let mut line = String::new();
        loop {
            line.clear();
            conn.stdout.read_line(&mut line).await.map_err(|e| TypesError::ToolExecutionError {
                tool_id: "read".into(),
                details: format!("Failed to read from sandboxd: {e}"),
            })?;

            if line.trim().is_empty() {
                continue;
            }

            let response: serde_json::Value =
                serde_json::from_str(line.trim()).map_err(|e| TypesError::ToolExecutionError {
                    tool_id: "parse".into(),
                    details: format!("Failed to parse sandboxd response: {e}"),
                })?;

            // Check if this response matches our request id
            if response.get("id").and_then(|v| v.as_u64()) == Some(id) {
                if let Some(error) = response.get("error").and_then(|v| v.as_str()) {
                    return Err(TypesError::ToolExecutionError {
                        tool_id: "sandboxd".into(),
                        details: error.to_string(),
                    });
                }
                return Ok(response);
            }

            // Mismatched id — log and keep reading
            warn!("Ignoring unexpected sandboxd response id");
        }
    }

    /// Gracefully shut down the daemon.
    pub async fn shutdown(&self) -> Result<(), TypesError> {
        let request = serde_json::json!({
            "method": "shutdown",
        });
        let line = serde_json::to_string(&request).map_err(|e| TypesError::ToolExecutionError {
            tool_id: "shutdown".into(),
            details: format!("Failed to serialize shutdown: {e}"),
        })?;

        let mut conn = self.conn.lock().await;
        conn.stdin.write_all(line.as_bytes()).await.ok();
        conn.stdin.write_all(b"\n").await.ok();
        conn.stdin.flush().await.ok();

        // Wait for the child to exit (with timeout)
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), conn.child.wait()).await;
        Ok(())
    }
}

impl SandboxdClient {
    /// 执行并向解析真实燃料测量（P2-9）。旧版守护进程响应无
    /// `fuel_consumed` 字段时返回 None（向后兼容）。
    async fn execute_with_measurement(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<(Vec<u8>, Option<sisyphus_types::ExecutionMeasurement>), TypesError> {
        let exec_start = tokio::time::Instant::now();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = serde_json::json!({
            "id": id,
            "method": "execute",
            "params": {
                "tool_id": tool_id,
                "args": args,
                "fuel_limit": fuel_limit,
            },
        });

        let response = self.send_request(id, &request).await?;

        // Decode base64 result data
        let data_b64 = response
            .get("result")
            .and_then(|r| r.get("data"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: "Missing 'data' field in sandboxd response".into(),
            })?;

        let data = base64_decode(data_b64).map_err(|e| TypesError::ToolExecutionError {
            tool_id: tool_id.to_string(),
            details: format!("Failed to decode sandboxd result: {e}"),
        })?;

        let measurement = response
            .get("result")
            .and_then(|r| r.get("fuel_consumed"))
            .and_then(|v| v.as_u64())
            .map(|fuel_consumed| sisyphus_types::ExecutionMeasurement {
                fuel_consumed,
                duration_ms: exec_start.elapsed().as_millis() as u64,
            });

        Ok((data, measurement))
    }
}

#[async_trait]
impl ToolExecutor for SandboxdClient {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        let (data, _) = self.execute_with_measurement(tool_id, args, fuel_limit).await?;
        Ok(data)
    }

    async fn execute_measured(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<(Vec<u8>, Option<sisyphus_types::ExecutionMeasurement>), TypesError> {
        self.execute_with_measurement(tool_id, args, fuel_limit).await
    }

    async fn list_tools(&self) -> Result<Vec<ToolMetadata>, TypesError> {
        // The daemon doesn't support listing tools directly.
        Ok(Vec::new())
    }

    fn as_cache_invalidatable(&self) -> Option<&dyn CacheInvalidatable> {
        None
    }
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    const DECODE: [i8; 256] = {
        let mut table = [-1i8; 256];
        let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < chars.len() {
            table[chars[i] as usize] = i as i8;
            i += 1;
        }
        table
    };

    let input = input.trim_end_matches('=');
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;

    for &byte in input.as_bytes() {
        let value = DECODE[byte as usize];
        if value < 0 {
            continue;
        }
        buffer = (buffer << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }

    Ok(output)
}
