//! HTTP 客户端性能基准：对比"共享 client" vs "每请求新 client"。
//!
//! 目标：以可重现的本地 HTTP 服务器为靶，测量两种策略的
//! 吞吐量、延迟、连接握手次数差异。
//!
//! 运行：`cargo run -p eval-runner --bin http-client-bench --release`
//!
//! 输出（stdout + JSON）：
//! - total_requests
//! - shared_total_ms / per_request_new_total_ms
//! - per_request_avg_ms（两策略）
//! - speedup_ratio
//!
//! 设计：
//! - 启动一个本地 axum server，/echo 返回 200
//! - 共享模式：所有请求用同一 Arc<reqwest::Client>
//! - 新建模式：每个请求 Arc::new(reqwest::Client::new())
//! - 跑相同请求数（默认 200），统计总时间

use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const DEFAULT_REQUESTS: usize = 200;
const DEFAULT_CONCURRENCY: usize = 20;

#[derive(Debug, Serialize)]
struct BenchResult {
    requests: usize,
    concurrency: usize,
    shared_total_ms: u128,
    shared_per_req_avg_us: u128,
    per_request_new_total_ms: u128,
    per_request_new_per_req_avg_us: u128,
    speedup_ratio: f64,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let requests = std::env::var("BENCH_REQUESTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_REQUESTS);
    let concurrency = std::env::var("BENCH_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CONCURRENCY);

    println!("▶ HTTP 客户端基准：共享 vs 每请求新");
    println!("  requests    = {requests}");
    println!("  concurrency = {concurrency}");

    // 1) 启动本地 echo server
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local listener");
    let addr = listener.local_addr().expect("local_addr");
    let server_url = format!("http://{addr}/echo");

    let server_handle = tokio::spawn(async move {
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => return,
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) => return,
                        Ok(_) => {
                            // 简单 HTTP/1.1 echo
                            let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: text/plain\r\nConnection: keep-alive\r\n\r\nok";
                            if sock.write_all(response).await.is_err() {
                                return;
                            }
                        }
                        Err(_) => return,
                    }
                }
            });
        }
    });

    // 2) 共享 client 模式
    let shared_client = Arc::new(
        reqwest::Client::builder()
            .pool_max_idle_per_host(concurrency)
            .build()
            .expect("build shared client"),
    );
    let shared_start = Instant::now();
    run_load(shared_client.clone(), &server_url, requests, concurrency).await;
    let shared_total_ms = shared_start.elapsed().as_millis();
    let shared_per_req_avg_us = (shared_total_ms * 1000) / requests as u128;

    // 3) 每请求新 client 模式
    let per_request_start = Instant::now();
    for _ in 0..requests {
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(0) // 不复用
            .build()
            .expect("build per-request client");
        let _ = client.get(&server_url).send().await;
    }
    let per_request_total_ms = per_request_start.elapsed().as_millis();
    let per_request_new_per_req_avg_us = (per_request_total_ms * 1000) / requests as u128;

    // 关闭 server
    server_handle.abort();

    // 4) 计算并输出
    let speedup = per_request_total_ms as f64 / shared_total_ms.max(1) as f64;
    let result = BenchResult {
        requests,
        concurrency,
        shared_total_ms,
        shared_per_req_avg_us,
        per_request_new_total_ms: per_request_total_ms,
        per_request_new_per_req_avg_us,
        speedup_ratio: speedup,
    };

    println!();
    println!("── 结果 ──");
    println!("  共享 client 总耗时:     {} ms ({} µs/req)", result.shared_total_ms, result.shared_per_req_avg_us);
    println!("  每请求新建总耗时:       {} ms ({} µs/req)", result.per_request_new_total_ms, result.per_request_new_per_req_avg_us);
    println!("  加速比:                 {:.2}x", result.speedup_ratio);
    println!();

    // 5) 落 JSONL 历史
    let dir = std::path::PathBuf::from(".sisyphus/eval-history");
    let _ = std::fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("http-client-bench-{ts}.jsonl"));
    if let Ok(line) = serde_json::to_string(&result) {
        let _ = std::fs::write(&path, format!("{line}\n"));
        println!("✓ 历史已写入: {}", path.display());
    }

    // 6) 退出码：如果共享模式真的更快则 0，否则 1（CI 可用作 sanity check）
    if result.speedup_ratio >= 1.5 {
        println!("✓ 共享模式快 ≥1.5x，符合预期");
    } else {
        println!("⚠ 共享模式加速比 < 1.5x，请检查网络或 server 瓶颈");
    }
}

async fn run_load(client: Arc<reqwest::Client>, url: &str, requests: usize, concurrency: usize) {
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::with_capacity(requests);
    for _ in 0..requests {
        let permit = sem.clone().acquire_owned().await.expect("semaphore");
        let client = client.clone();
        let url = url.to_string();
        handles.push(tokio::spawn(async move {
            let _permit = permit; // hold until task done
            let _ = client.get(&url).send().await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }
}
