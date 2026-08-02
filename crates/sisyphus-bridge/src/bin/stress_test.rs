//! Bridge stress/load testing binary.
//!
//! Usage:
//!   cargo run -p sisyphus-bridge --bin stress_test [ADDR] [CONCURRENCY] [ITERATIONS]
//!   cargo run -p sisyphus-bridge --bin stress_test --tool [ADDR] [CONCURRENCY] [ITERATIONS]
//!
//! ADDR defaults to http://[::1]:50051.
//! With `--quick` (or when CONCURRENCY=1), runs a minimal smoke test (1x1).
//! With `--tool`, issues `ExecuteTool` requests instead of `ExecuteSop`; this
//! exercises the chaos-fault injection path (`maybe_inject_chaos_fault`), which
//! only runs in front of tool execution.
//! Quick mode is suitable for CI to verify the binary compiles and connects.

use sisyphus_bridge_api::sisyphus_bridge_client::SisyphusBridgeClient;
use sisyphus_bridge_api::{ExecuteSopRequest, ExecuteToolRequest};
use std::time::Instant;
use tokio::task::JoinSet;
use tonic::Request;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Find the first positional arg that is not a flag (--quick/-q/--tool).
    // This allows `cargo run --bin stress_test -- --quick` to work correctly:
    // --quick triggers quick mode, while address falls back to default.
    let is_quick = args.iter().any(|a| a == "--quick" || a == "-q");
    let is_tool = args.iter().any(|a| a == "--tool" || a == "-t");
    let pos_args: Vec<&String> = args
        .iter()
        .skip(1) // skip binary name
        .filter(|a| !a.starts_with('-'))
        .collect();

    let addr = pos_args
        .first()
        .map(|s| (*s).clone())
        .unwrap_or_else(|| "http://[::1]:50051".to_string());

    let (concurrency, iterations) = if is_quick {
        (1, 1)
    } else {
        let c: usize = pos_args.get(1).and_then(|s| s.parse().ok()).unwrap_or(50);
        let i: usize = pos_args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);
        (c, i)
    };

    let channel = tonic::transport::Channel::from_shared(addr.clone())
        .map_err(|e| format!("Invalid address {addr}: {e}"))?
        .connect()
        .await?;

    let client = SisyphusBridgeClient::new(channel);
    let mut set = JoinSet::new();

    let total = concurrency * iterations;
    eprintln!(
        "🚀 Stress test: addr={addr} mode={} concurrency={concurrency} total_req={total}",
        if is_tool { "tool" } else { "sop" },
    );
    let start = Instant::now();

    for _ in 0..concurrency {
        let mut client = client.clone();
        set.spawn(async move {
            for _ in 0..iterations {
                if is_tool {
                    let request = Request::new(ExecuteToolRequest {
                        tool_id: "echo".into(),
                        args_json: "{}".into(),
                        fuel_limit: 0,
                    });
                    let _ = client.execute_tool(request).await;
                } else {
                    let request = Request::new(ExecuteSopRequest {
                        sop_id: "hello".into(),
                        args: std::collections::HashMap::new(),
                    });
                    let _ = client.execute_sop(request).await;
                }
            }
        });
    }

    while set.join_next().await.is_some() {}

    let elapsed = start.elapsed();
    println!(
        "✅ Stress test completed: {} requests in {:?} ({:.0} req/s)",
        total,
        elapsed,
        total as f64 / elapsed.as_secs_f64()
    );

    Ok(())
}
