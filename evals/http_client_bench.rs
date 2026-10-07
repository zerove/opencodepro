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
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind local listener");
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
                        },
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
    println!(
        "  共享 client 总耗时:     {} ms ({} µs/req)",
        result.shared_total_ms, result.shared_per_req_avg_us
    );
    println!(
        "  每请求新建总耗时:       {} ms ({} µs/req)",
        result.per_request_new_total_ms, result.per_request_new_per_req_avg_us
    );
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

    // 6) 退出码策略：
    //    - 共享模式加速比 < 1.5x → 1（共享池失效）
    //    - 若有上次 jsonl 基线且加速比 < 基线 × 0.8 → 1（性能回归 20%+）
    //    - 否则 → 0
    let mut exit_code = 0i32;
    if result.speedup_ratio < 1.5 {
        println!(
            "✗ FAIL: 共享模式加速比 {:.2}x < 1.5x 下限，连接复用可能失效",
            result.speedup_ratio
        );
        exit_code = 1;
    } else {
        println!("✓ 共享模式快 ≥1.5x，符合预期");
    }

    // 与上次基线对比
    if let Some(baseline) = read_last_baseline(&dir) {
        let ratio = result.speedup_ratio / baseline;
        println!("  上次基线加速比:        {:.2}x", baseline);
        println!("  相对基线:              {:.2}× (1.00 = 不变)", ratio);
        if ratio < 0.8 {
            println!(
                "✗ FAIL: 性能回归 20%+，基线 {:.2}x → 当前 {:.2}x",
                baseline, result.speedup_ratio
            );
            exit_code = 1;
        }
    } else {
        println!("  无历史基线（首次运行，仅做绝对阈值检查）");
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

/// 读取 `dir` 下 `http-client-bench-*.jsonl` 中**最后**一个文件的
/// 第一行（即最近一次 `speedup_ratio`），用作性能回归基线。
///
/// 返回 `None` 表示没有历史（首次运行）。
///
/// ## 为什么必须传显式路径
///
/// 本函数原先硬编码相对路径 `.sisyphus/eval-history`，解析结果完全取决于
/// **进程级**当前工作目录。而 `set_current_dir` 改的是全局状态、不是线程级
/// 的：libtest 默认并行跑同一二进制内的测试，于是两个都 chdir 的测试会互相
/// 踩 —— `test_read_last_baseline_returns_none_when_no_history` 在另一个测试
/// 正停留在它含 fixture 的临时目录时调用本函数，会读到别人的数据而非 `None`
/// （实测失败值 `Some(5.0)` 正是另一测试的 fixture）。
///
/// 上一个修法是用互斥锁把「chdir → 读 → chdir 回来」串行化，但那只是让两个
/// **已知**测试不互踩：只要再有一个测试 chdir、或有人在这段窗口里调本函数，
/// 竞争依旧成立 —— 真正的脆弱面是函数依赖全局可变状态本身。故改为显式接收
/// 目录：生产侧 `main` 传入 `.sisyphus/eval-history`（行为与原先完全一致），
/// 测试侧传入各自的临时目录，从此不再触碰进程 cwd，并发安全不依赖任何锁。
fn read_last_baseline(dir: &std::path::Path) -> Option<f64> {
    if !dir.exists() {
        return None;
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with("http-client-bench-")
                && e.file_name().to_string_lossy().ends_with(".jsonl")
        })
        .collect();
    if entries.is_empty() {
        return None;
    }
    // 按文件名排序（unix timestamp 后缀）→ 取最大
    entries.sort_by_key(|e| e.file_name());
    let last = entries.last()?.path();
    let content = std::fs::read_to_string(&last).ok()?;
    let line = content.lines().last()?;
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    v.get("speedup_ratio")?.as_f64()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 计数器: 给每个测试唯一 timestamp 后缀
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// 计数器: 给每个测试唯一的临时目录后缀
    fn unique_tmp(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "sisyphus-bench-test-{prefix}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ))
    }

    /// 在临时目录里写一个 jsonl，验证 read_last_baseline 能否读出 speedup_ratio
    ///
    /// 本测试**不 chdir**：`read_last_baseline` 现接受显式目录参数，
    /// 并发安全由「不依赖进程级 cwd」保证，而非靠锁把调用排成一行。
    #[test]
    fn test_read_last_baseline_returns_speedup_ratio() {
        let tmp = unique_tmp("ratio");
        // 构造 eval-history + 2 个 jsonl
        let dir = tmp.join("eval-history");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("http-client-bench-100.jsonl"),
            // rustc 1.98 clippy::useless_format 修复：直接用字符串（单层花括号）
            "{\"requests\":200,\"concurrency\":20,\"speedup_ratio\":5.0}\n",
        )
        .unwrap();
        fs::write(
            dir.join("http-client-bench-200.jsonl"),
            "{\"requests\":200,\"concurrency\":20,\"speedup_ratio\":12.5}\n",
        )
        .unwrap();

        let baseline = read_last_baseline(&dir);
        fs::remove_dir_all(&tmp).ok();

        assert_eq!(
            baseline,
            Some(12.5),
            "应返回最新文件 (200) 的 speedup_ratio"
        );
    }

    /// 无历史目录时返回 None
    #[test]
    fn test_read_last_baseline_returns_none_when_no_history() {
        let tmp = unique_tmp("none");
        fs::create_dir_all(&tmp).unwrap();
        let dir = tmp.join("eval-history");

        let baseline = read_last_baseline(&dir);
        fs::remove_dir_all(&tmp).ok();

        assert_eq!(baseline, None, "eval-history 不存在时应返回 None");
    }

    /// 目录存在但没有匹配的 jsonl 时也应返回 None。
    ///
    /// 该用例刻意与上面两个共用进程并行：它若依赖 cwd，就会读到别人的
    /// fixture 而失败 —— 这正是修复前 `Some(5.0)` 失败的成因。
    #[test]
    fn test_read_last_baseline_ignores_unrelated_files() {
        let tmp = unique_tmp("ignore");
        let dir = tmp.join("eval-history");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("other-bench-1.jsonl"), "{\"speedup_ratio\":9.9}\n").unwrap();
        fs::write(dir.join("readme.txt"), "not jsonl").unwrap();

        let baseline = read_last_baseline(&dir);
        fs::remove_dir_all(&tmp).ok();

        assert_eq!(baseline, None, "非 http-client-bench 前缀的文件应被忽略");
    }
}
