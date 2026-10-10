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
                // Added after `fuel_consumed`; older daemons omit it entirely,
                // so absence must degrade to `None` rather than zero.
                peak_memory_bytes: response
                    .get("result")
                    .and_then(|r| r.get("peak_memory_bytes"))
                    .and_then(|v| v.as_u64()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Locate the real `sisyphus-sandboxd` binary built alongside these tests.
    ///
    /// Test binaries live in `<target>/debug/deps/`, but `cargo build -p
    /// sisyphus-sandboxd` places the executable in `<target>/debug/`. Walk up
    /// until it actually shows up.
    fn sandboxd_bin() -> PathBuf {
        let mut dir = std::env::current_exe()
            .expect("test exe path")
            .parent()
            .expect("deps dir")
            .to_path_buf();
        loop {
            let candidate = dir.join("sisyphus-sandboxd");
            if candidate.exists() {
                return candidate;
            }
            assert!(
                dir.parent().is_some(),
                "sisyphus-sandboxd binary not found; build it with \
                 `cargo build -p sisyphus-sandboxd` first"
            );
            dir = dir.parent().expect("walk to target root").to_path_buf();
        }
    }

    /// Directory holding the workspace `.wasm` fixtures.
    fn tools_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .join("tools")
    }

    /// Shared multi-threaded runtime for the whole test module.
    ///
    /// `SandboxdClient` spawns a background task that owns the child's stdout, so
    /// the runtime must outlive the client. Building a fresh runtime per call would
    /// tear that reader down at the end of the first `block_on`, and the next call
    /// would fail with "A Tokio 1.x context was found, but it is being shutdown".
    fn rt() -> &'static tokio::runtime::Runtime {
        static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
        RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime")
        })
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        rt().block_on(f)
    }

    fn spawn_client() -> SandboxdClient {
        let dir = tools_dir();
        assert!(
            dir.join("hello.wasm").exists(),
            "tools fixtures missing at {dir:?}"
        );
        block_on(SandboxdClient::spawn(
            sandboxd_bin().to_str().expect("utf-8 path"),
            dir.to_str().expect("utf-8 path"),
            10,
        ))
        .expect("spawn sandboxd")
    }

    fn base64_encode(input: &[u8]) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(CHARS[(n >> 18) as usize & 63] as char);
            out.push(CHARS[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                CHARS[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                CHARS[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    /// The client's own base64 decoder must invert the daemon's encoder.
    ///
    /// This is the seam the daemon-side tests cannot see: they assert the wire
    /// bytes look right, this asserts the client can actually read them. A
    /// mismatch here would corrupt every result silently — the client would
    /// return garbage while the daemon looked perfectly healthy.
    #[test]
    fn base64_roundtrip_matches_the_daemon_encoder() {
        for payload in [
            &b""[..],
            b"a",
            b"ab",
            b"abc",
            b"Hello from WASM!",
            &[0u8, 255, 128, 1, 2, 3],
        ] {
            let encoded = base64_encode(payload);
            let decoded = base64_decode(&encoded).expect("decode");
            assert_eq!(
                decoded,
                payload.to_vec(),
                "roundtrip failed for {encoded:?}"
            );
        }
    }

    /// A missing measurement field must degrade to `None`, never to zero.
    ///
    /// This mirrors the daemon-side guarantee for daemons that omit
    /// `peak_memory_bytes`. Asserting it here matters because a silent change to
    /// `Some(0)` turns "not measured" into "measured as nothing" — the exact
    /// conflation `…_absent_not_zero_for_memoryless_module` prevents on the other
    /// side of the same wire.
    #[test]
    fn absent_measurement_fields_degrade_to_none() {
        let legacy = serde_json::json!({
            "id": 1,
            "result": { "data": base64_encode(b"Hello from WASM!") },
        });

        let measurement =
            legacy.get("result").and_then(|r| r.get("fuel_consumed")).and_then(|v| v.as_u64()).map(
                |fuel_consumed| sisyphus_types::ExecutionMeasurement {
                    fuel_consumed,
                    duration_ms: 0,
                    peak_memory_bytes: legacy
                        .get("result")
                        .and_then(|r| r.get("peak_memory_bytes"))
                        .and_then(|v| v.as_u64()),
                },
            );

        assert!(
            measurement.is_none(),
            "a legacy response with no fuel_consumed must yield None, got {measurement:?}"
        );

        let data_b64 = legacy["result"]["data"].as_str().expect("data field");
        assert_eq!(
            base64_decode(data_b64).expect("decode"),
            b"Hello from WASM!".to_vec()
        );
    }

    /// End-to-end over the real protocol: spawn the actual daemon binary, execute a
    /// real tool, and read the measurement this client surfaces.
    ///
    /// This is the integration boundary that `97a6468` moved across: the daemon no
    /// longer owns the execution path, so the fields parsed here are now produced
    /// by `sisyphus-sandbox`. Before this test nothing in the bridge ever launched
    /// the daemon, so a field rename on the daemon side would only have surfaced
    /// in production.
    #[test]
    fn executes_a_real_tool_through_the_real_daemon() {
        let client = spawn_client();

        let (data, measurement) = block_on(client.execute_measured("hello", vec![], 1_000_000))
            .expect("execute hello through sandboxd");

        assert_eq!(
            String::from_utf8(data).expect("utf-8"),
            "Hello from WASM!",
            "payload must survive the base64 roundtrip across the process boundary"
        );

        let measurement = measurement.expect("daemon must report a measurement");
        assert!(
            measurement.fuel_consumed > 0,
            "fuel_consumed must be a real value, got {}",
            measurement.fuel_consumed
        );
        assert_eq!(
            measurement.peak_memory_bytes,
            Some(65_536),
            "peak_memory_bytes must cross the process boundary intact"
        );
    }

    /// From the client side too: with fuel metering defeated (`fuel_limit =
    /// u64::MAX`), a runaway guest must still be stopped by the wall-clock bound
    /// rather than hanging the caller.
    #[test]
    fn a_runaway_guest_is_still_bounded_from_the_client() {
        let client = spawn_client();

        let outcome = block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(20),
                client.execute("infinite_loop", vec![], u64::MAX),
            )
            .await
        });

        let result = outcome.expect("daemon must answer within the wall-clock bound");
        let err = result.expect_err("runaway guest must not succeed");
        assert!(
            err.to_string().contains("ResourceExhausted"),
            "expected a ResourceExhausted shape, got: {err}"
        );

        block_on(client.shutdown()).expect("shutdown");
    }
}
