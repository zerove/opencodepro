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
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Spawn a sandboxd instance with a temporary tools directory.
/// If `copy_wasm` is provided, copies those (filename, bytes) pairs
/// into the tools directory so sandboxd can find them.
fn spawn_sandboxd() -> (
    Child,
    Box<dyn BufRead + Send>,
    Box<dyn Write + Send>,
    tempfile::TempDir,
) {
    let bin_path = env!("CARGO_BIN_EXE_sisyphus-sandboxd");

    // Create a temp directory for tools — kept alive by TempDir until dropped.
    let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");

    let mut child = Command::new(bin_path)
        .arg("--tools-dir")
        .arg(tmpdir.path())
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

    (child, stdout, stdin, tmpdir)
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
    let (mut child, mut stdout, mut stdin, _tmpdir) = spawn_sandboxd();

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
    let (mut child, mut stdout, mut stdin, _tmpdir) = spawn_sandboxd();

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
    let (mut child, mut stdout, mut stdin, _tmpdir) = spawn_sandboxd();

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
    let (mut child, mut stdout, mut stdin, _tmpdir) = spawn_sandboxd();

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
    let (mut child, mut stdout, mut stdin, _tmpdir) = spawn_sandboxd();

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

#[test]
fn test_execute_hello_wasm() {
    // Copy hello.wasm from the workspace tools directory to our temp dir
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_wasm = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("tools").join("hello.wasm"))
        .expect("Failed to resolve hello.wasm path");

    let (mut child, mut stdout, mut stdin, tmpdir) = spawn_sandboxd();

    // Copy hello.wasm into the temp tools directory
    let dest_wasm = tmpdir.path().join("hello.wasm");
    std::fs::copy(&src_wasm, &dest_wasm).expect("Failed to copy hello.wasm to temp dir");

    // Execute hello tool — it returns a base64-encoded pointer to "Hello from WASM!"
    let request = json_request(
        1,
        "execute",
        r#"{"tool_id":"hello","args":[],"fuel_limit":1000}"#,
    );
    let response = send_request(&mut stdout, &mut stdin, &request);

    let parsed: serde_json::Value = serde_json::from_str(&response).expect("Invalid JSON response");

    assert_eq!(parsed["id"], 1);
    assert!(
        parsed["result"]["data"].as_str().is_some(),
        "Expected base64 data in result, got: {response}"
    );

    // Decode the base64 response
    let b64 = parsed["result"]["data"].as_str().unwrap();
    let decoded = base64_simple_decode(b64);
    let output = String::from_utf8(decoded).expect("Output is not valid UTF-8");
    assert_eq!(output.trim(), "Hello from WASM!");

    let _ = child.kill();
    let _ = child.wait();
}

/// Minimal base64 decode (inverse of sandboxd's base64_encode).
fn base64_simple_decode(input: &str) -> Vec<u8> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [0u8; 256];
    for (i, &c) in CHARS.iter().enumerate() {
        rev[c as usize] = i as u8;
    }

    let input = input.trim_end_matches('=');
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.as_bytes().chunks(4) {
        let mut buf = [0u8; 4];
        for (i, &b) in chunk.iter().enumerate() {
            buf[i] = rev[b as usize];
        }
        let triple = ((buf[0] as u32) << 18)
            | ((buf[1] as u32) << 12)
            | ((buf[2] as u32) << 6)
            | (buf[3] as u32);
        out.push((triple >> 16) as u8);
        if chunk.len() > 2 {
            out.push((triple >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(triple as u8);
        }
    }
    out
}
