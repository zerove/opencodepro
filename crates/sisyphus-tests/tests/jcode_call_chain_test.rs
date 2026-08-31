use sisyphus_bridge::jcode_caller::DebugSocketToolCaller;
use sisyphus_tools_agent::jcode_executor::{JcodeToolCaller, JcodeToolExecutor};
use sisyphus_types::ToolExecutor;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

#[tokio::test]
#[ignore = "Requires running Jcode UDS server"]
async fn test_jcode_uds_call_chain() {
    // 1. Setup temporary directory for socket
    let dir = tempdir().expect("Failed to create temp dir");
    let socket_path = dir.path().join("jcode.sock").to_str().unwrap().to_string();

    // 2. Start a mock jcode server
    let socket_path_clone = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let listener = UnixListener::bind(&socket_path_clone).expect("Failed to bind UDS");
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.expect("Failed to read from socket");
            let request = String::from_utf8_lossy(&buf[..n]);

            if request.starts_with("tool:test_tool") {
                stream.write_all(b"mock_response_success").await.expect("Failed to write response");
            } else {
                stream.write_all(b"mock_response_error").await.expect("Failed to write response");
            }
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
