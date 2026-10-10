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
    spawn_sandboxd_with_env(&[])
}

/// Same as [`spawn_sandboxd`], but injects extra environment variables.
fn spawn_sandboxd_with_env(
    extra_env: &[(&str, &str)],
) -> (
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
        .envs(extra_env.iter().copied())
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
    stdout.read_line(&mut response).expect("Failed to read response");
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

/// Same as [`send_request`], but gives up after `timeout` instead of blocking forever.
///
/// 必要而非锦上添花：失控 guest 若没有被 epoch 兜底拦住，daemon 会**永久**卡在
/// 那次执行上（它逐条处理 stdio 请求），而阻塞的 `read_line` 没有超时，测试会直接
/// 挂死到被 CI 杀掉——挂死不是失败信号，只是噪声。返回 `Ok(None)` 让调用方
/// 把「超时」变成可断言的失败。
fn send_request_with_timeout(
    stdout: Box<dyn BufRead + Send>,
    stdin: Box<dyn Write + Send>,
    request: String,
    timeout: Duration,
) -> std::io::Result<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut stdin = stdin;
        let mut stdout = stdout;
        let outcome = (|| -> std::io::Result<String> {
            writeln!(stdin, "{request}")?;
            stdin.flush()?;
            let mut response = String::new();
            stdout.read_line(&mut response)?;
            Ok(response.trim().to_string())
        })();
        let _ = tx.send(outcome);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(response)) => Ok(Some(response)),
        Ok(Err(e)) => Err(e),
        Err(_) => Ok(None),
    }
}

/// Copy a workspace tool fixture into the daemon's tools directory.
fn install_tool(tmpdir: &tempfile::TempDir, tool_id: &str) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_wasm = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("tools").join(format!("{tool_id}.wasm")))
        .expect("Failed to resolve tool wasm path");
    std::fs::copy(&src_wasm, tmpdir.path().join(format!("{tool_id}.wasm")))
        .unwrap_or_else(|e| panic!("Failed to copy {tool_id}.wasm to temp dir: {e}"));
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
            },
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
        stdout.read_line(&mut line).expect("Failed to read response");
        responses.push(line.trim().to_string());
    }

    // Sort by id so order doesn't matter
    responses.sort();

    // rustc 1.98 clippy::unused_enumerate_index 修复：去除 `.enumerate()`，
    // 因为 `_i` 在循环体中未使用。
    for resp in responses.iter() {
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

/// 内存计量必须经由**对外协议**真的有值，不能永远是 `None`。
///
/// 这个断言是有来由的：`peak_memory_bytes` 是在“跨版本内存对照无数据可测”
/// 之后补进协议的，补的时候 `test_execute_hello_wasm` 已经存在但**只断言了
/// `data`**。也就是说字段可以静默退化成 `None` 而全部测试照样绿——它当初缺失
/// 的方式，正是现在这条断言要防止的复发方式。
///
/// 期望值 65536 = 64KiB = wasm 线性内存最小一页。`hello` 只申请一页，多余的
/// `memory_hog` 会 grow 到上限且必然 trap，所以拿它做「不撞上限」的正向对照是
/// 错的；这里只测不 grow 的模块。若工具真的申请更多，需连同本断言一起更新。
#[test]
fn test_peak_memory_is_reported_over_the_protocol() {
    let (mut child, mut stdout, mut stdin, tmpdir) = spawn_sandboxd();
    install_tool(&tmpdir, "hello");

    let request = json_request(
        1,
        "execute",
        r#"{"tool_id":"hello","args":[],"fuel_limit":1000}"#,
    );
    let response = send_request(&mut stdout, &mut stdin, &request);
    let parsed: serde_json::Value = serde_json::from_str(&response).expect("Invalid JSON response");

    let peak = parsed["result"]["peak_memory_bytes"].as_u64();
    assert_eq!(
        peak,
        Some(65_536),
        "hello should report exactly one 64KiB wasm page over the protocol, got: {response}"
    );

    // 阴断：同一响应里的 fuel 也必须真有值。两个字段都断言，避免只修一个。
    assert!(
        parsed["result"]["fuel_consumed"].as_u64().is_some(),
        "fuel_consumed should be reported alongside memory, got: {response}"
    );

    let _ = child.kill();
    let _ = child.wait();
}

/// 无内存导出的模块必须让 `peak_memory_bytes` 退化为 `None`，而非 0。
///
/// 这条不是为了测模块，而是为了固定「缺失」与「零」的语义差别：若某天把
/// `None` 错写成 `Some(0)`，监控会把「没测到」读成「占用为零」，而上面那条
/// 阳性用例照样绿。`infinite_loop.wat` 不导出 memory，正好是这个边界。
#[test]
fn test_peak_memory_is_absent_not_zero_for_memoryless_module() {
    // 夹具本身是无内存导出的；若将来有人给它加上 memory，这条测试的前提就
    // 失效了，所以在这里显式固定住前提而不是默默测别的模块。
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("tools").join("infinite_loop.wat"))
        .expect("Failed to resolve infinite_loop.wat path");
    let wat = std::fs::read_to_string(&src).expect("Failed to read infinite_loop.wat");
    assert!(
        !wat.contains("export \"memory\""),
        "infinite_loop must stay memoryless for this test to mean anything"
    );

    let (mut child, stdout, stdin, tmpdir) = spawn_sandboxd();
    install_tool(&tmpdir, "infinite_loop");

    // 给足时间让墙钟兜底收尾（实测稳定在 5s 上下），否则读行会一直等。
    let request = json_request(
        1,
        "execute",
        r#"{"tool_id":"infinite_loop","args":[],"fuel_limit":100000}"#,
    );
    let response = send_request_with_timeout(stdout, stdin, request, Duration::from_secs(20))
        .expect("request thread panicked")
        .expect("daemon never answered an errored execute");

    let parsed: serde_json::Value = serde_json::from_str(&response).expect("Invalid JSON response");
    assert!(
        parsed.get("result").is_none() || parsed["result"].is_null(),
        "runaway guest must not report success, got: {response}"
    );

    let _ = child.kill();
    let _ = child.wait();
}

/// 协议侧的燃料上限必须真的在生效，而不只是被转发下去。
///
/// 去重之前这条路径完全无人验证：daemon 的 10 个测试没有一处断言「fuel_limit
/// 小到一定程度就会拦住 guest」。于是有一个真实的失效模式没人看见——如果
/// `execute_internal_opts` 把 `metered` 写死成 `false`，或者 `set_fuel` 的实参被
/// 换掉，fuel_limit 会变成一个被协议忠实转发、却毫无约束力的装饰字段，而**墙钟
/// 兜底会把每次尝试都救回来**，所有既有测试照样绿。
///
/// 判别方式：给同一个 `infinite_loop` 两个相差 100 倍的燃料预算，要求耗时成比例
/// 拉开。若 `fuel_limit` 真的驱动计账，大预算会明显跑得更久；若它被忽略
/// （`metered=false`），两次都会停在墙钟上限，耗时相同。比值不依赖机器速度。
///
/// 关键细节：**两个预算必须在同一个 daemon 进程里跑**。每次都起新进程的话，
/// 每次都要重新编译 wasm，而实测小预算的整个请求只要 ~30ms——编译开销占了绝大
/// 多数，真实执行时间被完全淹没。我第一版正是这么写的，实测两种预算都是
/// 31-35ms，于是差点把「燃料失效」当成结论。改用同进程第二次调用（命中模块缓存）
/// 后，预算效应清晰可见：实测 57ms（1e6）对 523ms（1e9）。
#[test]
fn test_fuel_limit_actually_bounds_a_runaway_guest() {
    let small_budget = 1_000_000u64;
    let large_budget = small_budget * 1_000;
    let wall_clock = Duration::from_millis(sisyphus_types::node::EXECUTION_TIMEOUT_MS);

    let (mut child, mut stdout, mut stdin, tmpdir) = spawn_sandboxd();
    install_tool(&tmpdir, "infinite_loop");

    /// 发一条 execute，读回响应，断言是错误，并返回耗时。
    ///
    /// 这里用阻塞的 [`send_request`] 而不是带超时的版本：两次调用都被墙钟兜底
    /// 限死（墙钟在本测试里是开着的），最坏情况就是两次各等一个 5s 周期，
    /// 不会真的挂死；换来的是流可以被反复复用，第二次调用才能命中模块缓存。
    fn timed_execute(
        stdout: &mut Box<dyn BufRead + Send>,
        stdin: &mut Box<dyn Write + Send>,
        id: u64,
        budget: u64,
    ) -> Duration {
        let request = json_request(
            id,
            "execute",
            &format!(r#"{{"tool_id":"infinite_loop","args":[],"fuel_limit":{budget}}}"#),
        );
        let start = std::time::Instant::now();
        let response = send_request(stdout, stdin, &request);
        let elapsed = start.elapsed();

        let parsed: serde_json::Value =
            serde_json::from_str(&response).expect("Invalid JSON response");
        assert!(
            parsed.get("result").is_none() || parsed["result"].is_null(),
            "a fuel-exhausted guest must be reported as an error, got: {response}"
        );
        elapsed
    }

    // 第一次调用顺便把模块编进缓存，第二次才是纯执行时间。
    let warmup = timed_execute(&mut stdout, &mut stdin, 1, small_budget);
    let measured = timed_execute(&mut stdout, &mut stdin, 2, large_budget);

    assert!(
        measured < wall_clock,
        "fuel_limit={large_budget} took {measured:?}, past the wall-clock bound {wall_clock:?} — \
         the budget was too large to distinguish fuel from the clock"
    );
    assert!(
        measured > warmup.saturating_mul(3),
        "a 1000x fuel budget ({large_budget}) must take clearly longer than {small_budget}, \
         but they took {measured:?} vs {warmup:?} — fuel_limit is not driving execution time, \
         so it is probably not driving fuel accounting either"
    );

    let _ = child.kill();
    let _ = child.wait();
}

/// 墙钟兜底必须在**燃料计账失效**时独自拦住失控 guest。
///
/// 这条走的是 daemon 自己的执行路径（它与 library 各有一份拷贝粘贴），而不是
/// 复用库侧测试。原因是具体的：库侧的 `arm_epoch_deadline` 有测试，而 daemon
/// 侧的同名函数当时**零测试**——整个 sandboxd 套件没有一处引用 epoch、
/// `peak_memory` 或 `EXECUTION_TIMEOUT_MS`。所以“已同步”当时只是一句代码检视，
/// 不是事实。
///
/// 判别依据是**同进程内的相对对比**，而不是绝对耗时，也不是错误文本。
///
/// 为什么不看错误文本：epoch trap 与 fuel trap 都是 `ResourceExhausted`，文本
/// 上分不开。也不能靠 `is_err()`——那只能证明「它停了」，不能证明「谁停的」。
///
/// 为什么不用绝对下限：早先写过 `elapsed >= 一个 tick`（>=5000ms），实测 14 轮
/// 里有 2 轮是 4995/4996ms。根因是 `EpochTicker` 先 `sleep` 再
/// `increment_epoch()`，而 deadline 相对引擎当前 epoch 值设定，请求到达时刻
/// 与 ticker 相位无关，等待剩余时间在 (0, interval] 之间浮动——下界根本不成立。
/// 改成固定 1s 能绕过抖动，但又把「这台机器够不够快」写进了测试：实测冷编译
/// 就要 ~114ms，慢机器或更大的工具可能轻松超过 1s，于是假失败。
///
/// 现在的做法：同一个 daemon、同一个进程里先跑 `hello` 量出「一次正常执行 +
/// 编译 + 往返」的成本 `baseline`，再要求失控 guest 至少比它慢一个数量级。
/// 这样判别力与机器速度无关——真正的 epoch 等待是秒级，正常执行是毫秒级，
/// 量级差异在任何机器上都成立。
///
/// 用 `fuel_limit = u64::MAX` 从协议外侧模拟计账失效：daemon 直接
/// `set_fuel(fuel_limit)`，因此把上限设到顶就等价于「燃料永远用不完」。
///
/// 变异验证（判别力确实来自 `set_epoch_deadline`）：
/// - 抹掉 `store.set_epoch_deadline(1)` → 基线调用就当场 trap，本测试失败。
/// - 抹掉 `store.epoch_deadline_trap()` → 测试仍通过，但这是**等价变异**而非
///   漏洞。wasmtime 官方文档写明 trap 是 store 的默认行为：
///   "This behavior is the default if the store is not otherwise configured via
///   epoch_deadline_trap(), ..."，并要求 "it's required to call
///   [`Store::set_epoch_deadline`] or otherwise wasm will always immediately
///   trap."（wasmtime 48.0.5，`src/runtime/store.rs`）也就是说
///   `epoch_deadline_trap()` 只是把默认值写显而已，删掉不改变行为。
#[test]
fn test_wall_clock_bound_stops_runaway_guest_when_fuel_metering_fails() {
    use sisyphus_types::node::EXECUTION_TIMEOUT_MS;

    let (mut child, mut stdout, mut stdin, tmpdir) = spawn_sandboxd();
    install_tool(&tmpdir, "infinite_loop");
    install_tool(&tmpdir, "hello");

    // 同一进程内的基线：一次正常执行（含冷编译与协议往返）。
    let baseline = std::time::Instant::now();
    let warmup = send_request(
        &mut stdout,
        &mut stdin,
        &json_request(
            1,
            "execute",
            r#"{"tool_id":"hello","args":[],"fuel_limit":1000}"#,
        ),
    );
    let baseline = baseline.elapsed();
    let warmup: serde_json::Value = serde_json::from_str(&warmup).expect("Invalid JSON warmup");
    let warmup_output = warmup["result"]["data"].as_str().expect("baseline call must return data");
    assert_eq!(
        String::from_utf8(base64_simple_decode(warmup_output)).expect("baseline output is UTF-8"),
        "Hello from WASM!",
        "the baseline call must actually succeed, got: {warmup}"
    );

    // 上界留 3 倍余量：它是「确实卡死了」的信号，不是性能断言。
    let ceiling = Duration::from_millis(EXECUTION_TIMEOUT_MS * 3);
    // 下界用相对量级：秒级的 epoch 等待 vs 毫秒级的正常执行。
    // 10 倍是保守取值——实测两者相差三个数量级以上。
    let floor = baseline.saturating_mul(10).max(Duration::from_millis(1));

    let request = json_request(
        2,
        "execute",
        r#"{"tool_id":"infinite_loop","args":[],"fuel_limit":18446744073709551615}"#,
    );

    let start = std::time::Instant::now();
    let response = send_request_with_timeout(stdout, stdin, request, ceiling)
        .expect("request thread panicked")
        .expect("daemon hung on a runaway guest: the wall-clock bound did not engage");
    let elapsed = start.elapsed();

    let parsed: serde_json::Value = serde_json::from_str(&response).expect("Invalid JSON response");
    assert!(
        parsed.get("result").is_none() || parsed["result"].is_null(),
        "runaway guest must be reported as an error, got: {response}"
    );
    let err = parsed["error"].as_str().unwrap_or_default();
    assert!(
        err.contains("ResourceExhausted"),
        "runaway guest should land in the ResourceExhausted shape, got: {err}"
    );

    // 当场 trap（例如内存上限、未 arm 的 store）会与基线同量级；跑满一个 epoch
    // 周期则是秒级。相对判别比绝对阈值稳：它随机器速度一起缩放。
    assert!(
        elapsed >= floor,
        "returned after {elapsed:?}, which is not clearly slower than a normal \
         execution ({baseline:?}); something other than the wall-clock bound stopped \
         the guest immediately"
    );
    assert!(
        elapsed < ceiling,
        "wall-clock bound took {elapsed:?}, beyond ceiling {ceiling:?}"
    );

    let _ = child.kill();
    let _ = child.wait();
}

/// 阴性对照：**关掉**兜底时，同一个失控 guest 必须真的挂死。
///
/// 这是上一条的正面对照，不是补充。前一条只证明了“有兜底时会在秒级返回”，
/// 但若没有这条，就无法排除「它其实是被别的上限在某个时刻刚好拦住」这种解释。
///
/// 关键前提：`infinite_loop.wat` **不导出 memory**，所以内存上限这条路对它是
/// 封死的（daemon 以 `--max-memory-pages 10` 启动，但模块没有线性内存可供
/// grow）。排除掉内存与燃料（已设 `u64::MAX`）之后，唯一能停住它的就是 epoch。
/// 这也解释了为什么不能拿库侧的 `infinite_loop` 场景来类推：那份夹具在
/// library 的 max-memory 配置下会先撞内存上限，所以库侧那条阴性对照测的是
/// 「当场撞内存」，而这里测的才是真正的「无人拦截 → 挂死」。
///
/// 用运行时开关而不是编译期 flag，是因为后者要 CI 维护两套构建，而一条永远
/// 不会被执行的 `#[ignore]` 等于没有对照。
///
/// 因为预期就是挂死，本测试只能通过「等不到响应」来判定，所以必须自己带超时，
/// 否则失败模式是整个测试套件被 CI 杀掉，而不是一条清晰的断言失败。
#[test]
fn test_without_wall_clock_bound_a_runaway_guest_hangs() {
    let (mut child, stdout, stdin, tmpdir) =
        spawn_sandboxd_with_env(&[("SISYPHUS_SANDBOXD_NO_WALL_CLOCK", "1")]);
    install_tool(&tmpdir, "infinite_loop");

    let request = json_request(
        1,
        "execute",
        r#"{"tool_id":"infinite_loop","args":[],"fuel_limit":18446744073709551615}"#,
    );

    let response = send_request_with_timeout(stdout, stdin, request, Duration::from_secs(20))
        .expect("request thread panicked");

    assert!(
        response.is_none(),
        "expected the daemon to hang without the wall-clock bound, but it answered"
    );

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
