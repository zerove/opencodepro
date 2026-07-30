//! Integration tests for `sisyphus-sandboxd` — the standalone WASM sandbox daemon.
//!
//! These tests spawn the sandboxd binary as a subprocess and communicate with
//! it over JSON-over-stdio IPC, validating the full protocol lifecycle:
//! ping/pong, tool execution (error path), unknown method, and shutdown.
//!
//! # CI note
//!
//! This test crate depends on `sisyphus-sandbox` (wasmtime), so it is excluded
//! from `--workspace` commands. Run with `cargo test -p sisyphus-sandboxd`.

use std::io::{BufRead, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Spawn a sandboxd instance with an empty tools directory.
fn spawn_sandboxd() -> (Child, Box<dyn BufRead + Send>, Box<dyn Write + Send>) {
    let bin_path = env!("CARGO_BIN_EXE_sisyphus-sandboxd");

    // Create an empty temp directory for tools so the executor initializes.
    let tmpdir = std::env::temp_dir().join(format!("sandboxd_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmpdir);

    let mut child = Command::new(bin_path)
        .arg("--tools-dir")
        .arg(&tmpdir)
        .arg("--max-memory-pages")
        .arg("10")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn sandboxd");

    let stdin = Box::new(child.stdin.take().expect("stdin not available"));
    let stdout = Box::new(std::io::BufReader::new(
        child.stdout.take().expect("stdout not available"),
    ));

    // Clean up temp dir when process exits.
    let _ = std::fs::remove_dir_all(&tmpdir);

    (child, stdout, stdin)
}

/// Send a JSON request line to sandboxd and read the response line.
fn send_request(
    stdout: &mut Box<dyn BufRead + Send>,
    stdin: &mut Box<dyn Write + Send>,
    request: &str,
) -> String {
    writeln!(stdin, "{request}").expect("Failed to write request");
    stdin.flush().expect("Failed to flush stdin");

    let mut response = String::new();
    stdout
        .read_line(&mut response)
        .expect("Failed to read response");
    response.trim().to_string()
}

/// Build a JSON-RPC-like request string.
fn json_request(id: u64, method: &str, params: &str) -> String {
    if params.is_empty() {
        format!(r#"{{"id":{id},"method":"{method}"}}"#)
    } else {
        format!(r#"{{"id":{id},"method":"{method}","params":{params}}}"#)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[test]
fn test_ping_pong() {
    let (mut child, mut stdout, mut stdin) = spawn_sandboxd();

    let request = json_request(1, "ping", "");
    let response = send_request(&mut stdout, &mut stdin, &request);

    // Should respond with {"id":1,"result":{"pong":true}}
    let parsed: serde_json::Value = serde_json::from_str(&response).expect("Invalid JSON response");

    assert_eq!(parsed["id"], 1);
    assert_eq!(parsed["result"]["pong"], true);
    assert!(parsed["error"].is_null() || parsed.get("error").is_none());

    // Clean shutdown
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_unknown_method() {
    let (mut child, mut stdout, mut stdin) = spawn_sandboxd();

    let request = json_request(42, "nonexistent_method", "");
    let response = send_request(&mut stdout, &mut stdin, &request);

    let parsed: serde_json::Value = serde_json::from_str(&response).expect("Invalid JSON response");

    assert_eq!(parsed["id"], 42);
    assert!(parsed["error"].as_str().unwrap().contains("Unknown method"));

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_execute_missing_tool() {
    let (mut child, mut stdout, mut stdin) = spawn_sandboxd();

    let request = json_request(
        7,
        "execute",
        r#"{"tool_id":"nonexistent_tool","args":[],"fuel_limit":1000}"#,
    );
    let response = send_request(&mut stdout, &mut stdin, &request);

    let parsed: serde_json::Value = serde_json::from_str(&response).expect("Invalid JSON response");

    assert_eq!(parsed["id"], 7);
    // Should return an error since the tool doesn't exist
    assert!(
        parsed["error"].as_str().is_some(),
        "Expected error for missing tool, got: {response}"
    );

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_shutdown_notification() {
    let (mut child, mut stdout, mut stdin) = spawn_sandboxd();

    // Send a ping to verify the daemon is alive first
    let ping_req = json_request(1, "ping", "");
    let pong = send_request(&mut stdout, &mut stdin, &ping_req);
    let parsed: serde_json::Value = serde_json::from_str(&pong).expect("Invalid JSON");
    assert_eq!(parsed["result"]["pong"], true);

    // Send shutdown notification (no id = notification)
    writeln!(stdin, r#"{{"method":"shutdown"}}"#).expect("Failed to write shutdown");
    stdin.flush().expect("Failed to flush");

    // Wait for process to exit cleanly (with timeout)
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(5) {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break None,
        }
    };

    assert!(
        status.is_some(),
        "Sandboxd did not exit within 5s after shutdown"
    );
    assert!(
        status.unwrap().success(),
        "Sandboxd exited with non-zero status"
    );
}

#[test]
fn test_concurrent_requests() {
    let (mut child, mut stdout, mut stdin) = spawn_sandboxd();

    // Send two pings without waiting for the first response
    let req1 = json_request(1, "ping", "");
    let req2 = json_request(2, "ping", "");

    writeln!(stdin, "{req1}").expect("write req1");
    writeln!(stdin, "{req2}").expect("write req2");
    stdin.flush().expect("flush");

    // Read both responses (order may vary, but both should come back)
    let mut responses = Vec::new();
    let mut line = String::new();
    for _ in 0..2 {
        line.clear();
        stdout
            .read_line(&mut line)
            .expect("Failed to read response");
        responses.push(line.trim().to_string());
    }

    // Sort by id so order doesn't matter
    responses.sort();

    for (_i, resp) in responses.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(resp).expect("Invalid JSON response");
        assert_eq!(parsed["result"]["pong"], true);
        // Responses should have ids 1 or 2
        let id = parsed["id"].as_u64().unwrap();
        assert!(id == 1 || id == 2, "Unexpected id: {id}");
    }

    let _ = child.kill();
    let _ = child.wait();
}
