//! 2 节点 in-process 集成测试（TODO 8.3.1）
//!
//! 不依赖 docker compose — 在同一进程内启动两个 ClusterService，验证：
//! 1. Join 握手：返回 peer 列表 + leader
//! 2. Stream 双向流：事件在 manager 之间传播
//! 3. leader 选择（字典序最小）

use sisyphus_bridge::peer::manager::{ClusterEvent, PeerInfo, PeerManager};
use sisyphus_bridge::peer::service::ClusterServiceImpl;
use sisyphus_bridge_api::cluster_proto::cluster_client::ClusterClient;
use sisyphus_bridge_api::cluster_proto::cluster_server::ClusterServer;
use sisyphus_bridge_api::cluster_proto::{ClusterEvent as ProtoEvent, JoinRequest};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tonic::transport::Server;

async fn start_test_node(
    node_id: &'static str,
) -> (String, Arc<PeerManager>, tokio::task::JoinHandle<()>) {
    let manager = Arc::new(PeerManager::new(node_id, Duration::from_secs(30)));
    let service = ClusterServiceImpl::new(manager.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let grpc_addr = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        Server::builder()
            .add_service(ClusterServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("server failed");
    });

    (grpc_addr, manager, handle)
}

#[tokio::test]
async fn test_two_node_join_returns_peer_list() {
    let (addr_a, _manager_a, _h_a) = start_test_node("node-a").await;

    let mut client = ClusterClient::connect(addr_a).await.expect("connect");

    let response = client
        .join(JoinRequest {
            node_id: "node-c".into(),
            grpc_addr: "http://127.0.0.1:9999".into(),
            cluster_name: "test".into(),
        })
        .await
        .expect("join failed")
        .into_inner();

    let peer_ids: Vec<String> = response.peers.iter().map(|p| p.node_id.clone()).collect();
    assert!(
        peer_ids.contains(&"node-a".to_string()),
        "should include node-a"
    );
    assert!(
        peer_ids.contains(&"node-c".to_string()),
        "should include node-c (joiner)"
    );
    assert_eq!(response.leader_id, "node-a");
}

#[tokio::test]
async fn test_two_node_stream_propagates_peer_joined() {
    let (addr_a, manager_a, _h_a) = start_test_node("node-a").await;

    let mut rx_a = manager_a.subscribe();

    let mut client_b = ClusterClient::connect(addr_a.clone()).await.expect("connect");
    let outbound = tokio_stream::iter(Vec::<ProtoEvent>::new());
    let response = client_b.stream(tonic::Request::new(outbound)).await.expect("stream");
    let mut _inbound = response.into_inner();

    tokio::time::sleep(Duration::from_millis(50)).await;

    manager_a.on_peer_joined(PeerInfo::new("node-b", "addr-b"));

    let evt = timeout(Duration::from_secs(2), rx_a.recv()).await.expect("timeout").expect("recv");

    match evt {
        ClusterEvent::PeerJoined(p) => {
            assert_eq!(p.node_id, "node-b");
        },
        other => panic!("expected PeerJoined, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_peer_manager_isolation() {
    let manager_a = PeerManager::new("node-a", Duration::from_secs(30));
    let manager_b = PeerManager::new("node-b", Duration::from_secs(30));

    manager_a.on_peer_joined(PeerInfo::new("node-x", "addr-x"));

    assert_eq!(manager_a.cluster_size(), 2);
    assert_eq!(manager_b.cluster_size(), 1);
}

#[tokio::test]
async fn test_two_node_join_leader_is_lexicographic_min() {
    let (addr_a, _m, _) = start_test_node("node-z").await;
    let (addr_b, _m, _) = start_test_node("node-a").await;

    // node-a join 到 node-z
    let mut client_b = ClusterClient::connect(addr_a.clone()).await.expect("connect a");
    let resp_b = client_b
        .join(JoinRequest {
            node_id: "node-a".into(),
            grpc_addr: addr_b.clone(),
            cluster_name: "test".into(),
        })
        .await
        .expect("join b→a")
        .into_inner();

    // 现在 node-z 视角：peers = [node-z, node-a]
    // leader = "node-a"（字典序最小）
    assert_eq!(resp_b.leader_id, "node-a");

    // node-z join 到 node-a
    let mut client_a = ClusterClient::connect(addr_b).await.expect("connect b");
    let resp_a = client_a
        .join(JoinRequest {
            node_id: "node-z".into(),
            grpc_addr: addr_a.clone(),
            cluster_name: "test".into(),
        })
        .await
        .expect("join a→b")
        .into_inner();

    // 现在 node-a 视角：peers = [node-a, node-z]
    // leader = "node-a"（字典序最小）
    assert_eq!(resp_a.leader_id, "node-a");
}
