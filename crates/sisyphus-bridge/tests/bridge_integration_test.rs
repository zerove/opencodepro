use sisyphus_bridge::bridge_service::MySisyphusBridge;
use sisyphus_bridge_api::sisyphus_bridge_client::SisyphusBridgeClient;
use sisyphus_bridge_api::sisyphus_bridge_server::SisyphusBridgeServer;
use sisyphus_bridge_api::{ExecuteSopRequest, HealthCheckRequest};
use sisyphus_tools_llm::metis::DummyLlmClient;
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use tonic::transport::{Channel, Server};

#[tokio::test]
async fn test_bridge_health_check() {
    // 1. Setup Minimal Config
    let mut config = Config::default();
    config.cache.sqlite_path = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string(); // Dummy path
    let shared_config = Arc::new(RwLock::new(config));

    // 2. Initialize Bridge
    let bridge = MySisyphusBridge::new(shared_config, None)
        .await
        .expect("Failed to create bridge");

    // 3. Start Server on random port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind listener");
    let addr = listener.local_addr().expect("Failed to get local addr");
    let server_addr = format!("http://{}", addr);

    let bridge_clone = bridge.clone();
    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(SisyphusBridgeServer::new(bridge_clone))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("Server failed");
    });

    // 4. Create Client and Call HealthCheck
    let mut client = SisyphusBridgeClient::connect(server_addr.clone())
        .await
        .expect("Failed to connect client");

    let request = tonic::Request::new(HealthCheckRequest {});
    let response = client
        .health_check(request)
        .await
        .expect("HealthCheck call failed");

    let inner = response.into_inner();
    assert_eq!(inner.status, "SERVING");

    server_handle.abort();
}

#[tokio::test]
async fn test_bridge_execute_sop() {
    // 1. Setup Minimal Config
    let mut config = Config::default();
    config.cache.sqlite_path = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    let shared_config = Arc::new(RwLock::new(config));

    // 2. Initialize Bridge
    let bridge = MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)))
        .await
        .expect("Failed to create bridge");

    // 3. Inject a mock SOP into the kernel
    let sop_id = "test_sop".to_string();
    let mut graph = sisyphus_types::SOPGraph::new();
    let node = Arc::new(sisyphus_types::WasmSOPNode {
        name: "test_node".into(),
        tool_id: "echo".into(),
        executor: bridge.kernel().execution_manager.tool_router.clone(),
        fuel_limit: 100,
    });
    graph.add_node(node);
    bridge
        .kernel()
        .save_sop(&sop_id, &graph)
        .await
        .expect("Failed to save mock SOP");

    // 4. Start Server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get addr");
    let server_addr = format!("http://{}", addr);

    let bridge_clone = bridge.clone();
    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(SisyphusBridgeServer::new(bridge_clone))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("Server failed");
    });

    // 5. Call execute_sop
    let mut client = SisyphusBridgeClient::connect(server_addr)
        .await
        .expect("Failed to connect");
    let request = tonic::Request::new(ExecuteSopRequest {
        sop_id,
        args: std::collections::HashMap::new(),
    });
    let response = client
        .execute_sop(request)
        .await
        .expect("execute_sop call failed");

    let inner = response.into_inner();
    assert!(inner.status == "SUCCESS" || inner.status == "FAILED");

    server_handle.abort();
}
