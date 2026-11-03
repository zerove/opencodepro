use sisyphus_bridge::jcode_caller::DebugSocketToolCaller;
use sisyphus_tools_agent::jcode_executor::{JcodeToolCaller, JcodeToolExecutor};
use sisyphus_types::ToolExecutor;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

/// 历史忽略理由已过时：本用例唯一的外部依赖是 `sisyphus_bridge` 这个
/// crate。当年在受限机器上编译 `sisyphus-bridge`（sandbox/wasmtime 默认
/// 特性）会 `rustc-LLVM ERROR: out of memory`，因此标了 `#[ignore]`；
/// 现在依赖目标可以正常编译，用例也被本机 `cargo test -p sisyphus-tests`
/// 实际执行（首次执行即暴露 mock server 只 accept 一次的缺陷）。
///
/// 注意：CI 对 sisyphus-tests 只跑 `cargo test -p sisyphus-tests --lib`
/// （见 `.github/workflows/ci.yml` 的 test-integration job），所以本集成
/// 目标在 CI 中依旧不参与，只在本地/专用 runner 上执行。
#[tokio::test]
async fn test_jcode_uds_call_chain() {
    // 1. Setup temporary directory for socket
    let dir = tempdir().expect("Failed to create temp dir");
    let socket_path = dir.path().join("jcode.sock").to_str().unwrap().to_string();

    // 2. Start a mock jcode server
    let socket_path_clone = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let listener = UnixListener::bind(&socket_path_clone).expect("Failed to bind UDS");
        // `DebugSocketToolCaller::call_tool` 每次调用都会新建一条连接，mock
        // server 必须按预期调用次数循环 accept，并且**在响应后关闭写端**：
        // call_tool 用 `read_to_end` 等 EOF。历史上只 accept 一次就退出，
        // listener 随任务结束被 drop，第二次调用直接 `ECONNREFUSED`，
        // 3 次 100/200ms 退避重试也救不回。下方两个连接分别对应
        // step 4（直连 caller）与 step 5（经 JcodeToolExecutor）。
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.expect("Failed to read from socket");
            let request = String::from_utf8_lossy(&buf[..n]);

            if request.starts_with("tool:test_tool") {
                stream.write_all(b"mock_response_success").await.expect("Failed to write response");
            } else {
                stream.write_all(b"mock_response_error").await.expect("Failed to write response");
            }
            // 显式关闭写端，让客户端的 read_to_end 立刻拿到 EOF。
            let _ = stream.shutdown().await;
        }
    });

    // 3. Initialize the caller
    let caller = Arc::new(DebugSocketToolCaller::new(socket_path));

    // 4. Test direct caller
    let result = caller.call_tool("test_tool", b"{\"param\": 1}".to_vec()).await;
    assert!(result.is_ok(), "Caller should succeed: {:?}", result.err());
    assert_eq!(result.unwrap(), b"mock_response_success");

    // 5. Test via JcodeToolExecutor
    let executor = JcodeToolExecutor::new();
    executor.set_caller(caller);

    let exec_result = executor.execute("test_tool", b"{\"param\": 1}".to_vec(), 1000).await;
    assert!(
        exec_result.is_ok(),
        "Executor should succeed: {:?}",
        exec_result.err()
    );
    assert_eq!(exec_result.unwrap(), b"mock_response_success");

    server_handle.await.expect("Server handle failed");
}

#[tokio::test]
async fn test_jcode_connection_failure() {
    let dir = tempdir().expect("Failed to create temp dir");
    let socket_path = dir.path().join("non_existent.sock").to_str().unwrap().to_string();

    let caller = DebugSocketToolCaller::new(socket_path);
    let result = caller.call_tool("test_tool", b"".to_vec()).await;

    assert!(result.is_err(), "Should fail when socket doesn't exist");
    assert!(result.unwrap_err().contains("failed after 3 attempts"));
}
