//! ADR-013 断流钩子的每消息开销基准（默认忽略，按需显式运行）。
//!
//! 跑法：
//! ```text
//! cargo test -p sisyphus-bridge --test hook_overhead -- --ignored --nocapture
//! ```
//!
//! 为什么要留这个文件：ADR-013 的「代价」小节断言钩子让每条消息多付
//! 1.1–1.6 µs（约 3 倍）。那个数字如果只能靠一段已经被删掉的临时代码
//! 复现，它和 ADR 批评的那些「凭直觉写的声明」没有区别。
//!
//! ## 测的是什么
//!
//! 对照两种 outbound 流形态——也就是 ADR-013 引入断流钩子前后的差别：
//!
//! - (a) 裸 `UnboundedReceiverStream`：钩子不存在时的形态
//! - (b) `async_stream::stream!` 包一层 `select!`：当前生产形态
//!   （`service.rs` 的 `stream()` 就是这么写的）
//!
//! 关心的只是「每条消息多付出多少」，与绝对吞吐无关。
//!
//! ## 两个容易把测量做废的地方
//!
//! 1. **计时必须覆盖消费端。** `unbounded` 通道下 `send` 永不阻塞，只计
//!    `send` 循环等于在测生产者吞吐，**测不到消费端每条消息的 `select!`
//!    分派成本**——而那正是钩子的开销所在。第一版基准就栽在这里，测出
//!    「-21 ns/条」的负开销（纯噪声），差点据此断言钩子免费。
//! 2. **必须 `drop(tx)`。** 否则消费端的 `next()` 永远等不到 `None`，任务不结束。
//!
//! ## 已知局限
//!
//! - 载荷用 `u64` 而非真实 `ProtoEvent`：`select!` 的分派成本与元素类型无关，
//!   但**不含**真实 protobuf 序列化成本。若把序列化算进来，钩子的**相对**
//!   开销会显著变小（分母变大）。因此本基准是钩子开销的**上界**。
//! - 绝对值受机器负载影响，只在同机同负载下有意义。跨机器比较无意义。
//! - 这是 tokio 单线程内的微基准，不代表 gRPC 实际吞吐——真实路径上每条消息
//!   还要过编码、写 socket、系统调用，钩子占比会低得多。

use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;

const N: usize = 100_000;
const REPEAT: usize = 5;

/// 形态 (a)：裸流，无钩子。
async fn bench_bare() -> Duration {
    let (tx, rx) = mpsc::unbounded_channel::<u64>();
    let raw = UnboundedReceiverStream::new(rx);
    let done = tokio::spawn(async move {
        let mut raw = raw;
        let mut n = 0usize;
        while raw.next().await.is_some() {
            n += 1;
        }
        n
    });
    let t = Instant::now();
    for i in 0..N {
        tx.send(i as u64).unwrap();
    }
    // 必须 drop 才能让消费端的 `next()` 返回 None、任务结束——计时随之覆盖
    // 「消费端取完最后一条」的全过程。
    drop(tx);
    let _ = tokio::time::timeout(Duration::from_secs(30), done).await;
    t.elapsed()
}

/// 形态 (b)：当前生产形态——`async_stream::stream!` 包一层 `select!`。
///
/// 生产路径上 `stream_break` 永不发信号，因此断流分支永不触发；但每条消息
/// 都要过一次 `select!` 分派，这正是要测的开销。
async fn bench_hooked() -> Duration {
    let (tx, rx) = mpsc::unbounded_channel::<u64>();
    let (break_tx, mut break_rx) = broadcast::channel::<()>(1);
    // 保持 sender 存活：与生产一致（`self.stream_break` 一直在）。
    let _keep_break_tx = break_tx;
    let raw = UnboundedReceiverStream::new(rx);
    let hooked = async_stream::stream! {
        let mut raw = raw;
        loop {
            tokio::select! {
                biased;
                _ = break_rx.recv() => {
                    break;
                },
                item = raw.next() => match item {
                    // 与生产一致：gRPC 流的元素是 `Result<ProtoEvent, Status>`。
                    Some(evt) => yield Ok::<_, std::io::Error>(evt),
                    None => break,
                },
            }
        }
    };
    let mut pinned = Box::pin(hooked);
    let done = tokio::spawn(async move {
        let mut n = 0usize;
        while pinned.next().await.is_some() {
            n += 1;
        }
        n
    });
    let t = Instant::now();
    for i in 0..N {
        tx.send(i as u64).unwrap();
    }
    drop(tx);
    let _ = tokio::time::timeout(Duration::from_secs(30), done).await;
    t.elapsed()
}

/// 形态 (c)：**优化后的生产路径**——`UnboundedReceiverStream::map(Ok)`，无钩子。
///
/// 这一形态此前被当作「零成本」直接断言，没人测过。实测结论要谨慎：跨多次
/// 运行，(c) 相对裸流的差值在 **−6 ns 到 +109 ns** 之间来回摆动，而裸流自身
/// 也会在 **540–735 ns** 之间波动（约 35%）。也就是说 `.map` 的成本**落在本
/// 基准的噪声底之下，无法与裸流区分**——既不能宣称它免费，也不能宣称它要多
/// 付几十纳秒。单次运行的数字（含本文件内中位数 of 5）都不足以支撑这种断言。
///
/// 能站住的只有相对量级：优化移掉的 `select!` 分派是 +1120~+1571 ns/条，
/// 稳定在裸流的 3 倍以上，远超噪声；而 `.map` 相对它的量级可忽略。
async fn bench_map_only() -> Duration {
    let (tx, rx) = mpsc::unbounded_channel::<u64>();
    let mapped = UnboundedReceiverStream::new(rx).map(Ok::<_, std::io::Error>);
    let done = tokio::spawn(async move {
        let mut mapped = mapped;
        let mut n = 0usize;
        while mapped.next().await.is_some() {
            n += 1;
        }
        n
    });
    let t = Instant::now();
    for i in 0..N {
        tx.send(i as u64).unwrap();
    }
    drop(tx);
    let _ = tokio::time::timeout(Duration::from_secs(30), done).await;
    t.elapsed()
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

/// 打印结果而不设门限：这是**测量**而非断言。绝对值随机器而变，任何硬编码
/// 门限都会在别的机器上变成 flaky。用例只保证「三种形态都能跑完并出数」。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "耗时基准，按需手动运行；绝对值随机器而变，不适合 CI 断言"]
async fn measure_stream_hook_overhead() {
    // 预热，避免首次分配/首次 poll 计入。
    let _ = bench_bare().await;
    let _ = bench_hooked().await;
    let _ = bench_map_only().await;

    let mut bare = Vec::with_capacity(REPEAT);
    let mut hooked = Vec::with_capacity(REPEAT);
    let mut map_only = Vec::with_capacity(REPEAT);
    for _ in 0..REPEAT {
        bare.push(bench_bare().await);
        hooked.push(bench_hooked().await);
        map_only.push(bench_map_only().await);
    }

    let b = median(bare);
    let h = median(hooked);
    let m = median(map_only);
    let per = |d: Duration| d.as_nanos() as f64 / N as f64;
    let (per_bare, per_hooked, per_map) = (per(b), per(h), per(m));
    println!("\n=== ADR-013 断流钩子开销（每 {N} 条消息，中位数 of {REPEAT}）===");
    println!("(a) 裸流              : {b:?}");
    println!("(b) 带钩子流 (优化前) : {h:?}");
    println!("(c) .map(Ok) (优化后) : {m:?}");
    println!("\n每条消息:");
    println!("  (a) 裸流              : {per_bare:.1} ns");
    println!(
        "  (b) 带钩子流 (优化前) : {per_hooked:.1} ns  ({:+.1} ns vs 裸流)",
        per_hooked - per_bare
    );
    println!(
        "  (c) .map(Ok) (优化后) : {per_map:.1} ns  ({:+.1} ns vs 裸流)",
        per_map - per_bare
    );
    println!(
        "\n优化收益 ((b) -> (c)): 每条省下 {:.1} ns",
        per_hooked - per_map
    );
    println!(
        "相对开销 (b/a): {:.2}x, (c/a): {:.2}x",
        per_hooked / per_bare,
        per_map / per_bare
    );
    println!("\n注：载荷为 u64，不含 protobuf 序列化与 socket 开销，故这是上界。");
    println!(
        "(c) 的成本低于本基准噪声底（裸流自身跨运行波动可达 35%），\
         故 (c)-vs-(a) 的 {:+.1} ns",
        per_map - per_bare
    );
    println!("不能当作「.map 要付多少 ns」的结论；能站住的只有 (b) 显著高于 (a)。");
}
