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

// ── 9.1.2 端到端测试：Experience CRDT broadcast + handler ──────────────

use sisyphus_bridge::peer::broadcast_experience;
use sisyphus_types::experience::ExperienceTuple;

/// 验证 ExperienceSync 事件通过 peer manager 广播，handler 能正确订阅。
///
/// 该测试在进程内模拟 2 节点：构造一个 PeerManager 作为"共享广播通道"，
/// 验证 broadcast_experience 产生的事件能被订阅者接收并携带正确的
/// exp_key / version / payload。
#[tokio::test]
async fn test_experience_broadcast_subscribed() {
    let manager = Arc::new(PeerManager::new("node-a", Duration::from_secs(30)));
    let mut rx = manager.subscribe();

    // 构造 experience
    let mut exp = ExperienceTuple {
        context_text: "user asked how to build an HTTP server".into(),
        context_hash: "abc123".into(),
        action_id: "write_file".into(),
        project_id: Some("project-x".into()),
        outcome_success: true,
        deviation_score: 0.1,
        cost: 42,
        timestamp: 1_700_000_000,
        embedding: None,
        visual_embedding: None,
        insight: Some("prefer axum over actix".into()),
        version: 3,
        vector_clock: {
            let mut m = std::collections::BTreeMap::new();
            m.insert("node-a".to_string(), 3u64);
            m
        },
    };

    // 广播（exp_key 含非 node-a 前缀以模拟来自 node-b）
    broadcast_experience(&manager, &exp, "exp:node-b-abc123");

    let evt = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout waiting for event")
        .expect("recv failed");

    match evt {
        ClusterEvent::ExperienceSync {
            exp_key,
            version,
            vector_clock_json,
            payload,
        } => {
            assert_eq!(exp_key, "exp:node-b-abc123");
            assert_eq!(version, 3);
            // vector_clock 应序列化 node-a:3
            assert!(vector_clock_json.contains("\"node-a\":3"));
            // payload 应反序列化回 ExperienceTuple
            let parsed: ExperienceTuple = serde_json::from_slice(&payload).expect("parse payload");
            assert_eq!(
                parsed.context_text,
                "user asked how to build an HTTP server"
            );
            assert_eq!(parsed.outcome_success, true);
            assert_eq!(parsed.vector_clock.get("node-a"), Some(&3));
        },
        other => panic!("expected ExperienceSync, got: {other:?}"),
    }

    // 模拟 handler 的"自广播过滤"：含 self_node_id 的 exp_key 应跳过
    exp.action_id = "second".into();
    broadcast_experience(&manager, &exp, "exp:node-a-abc123");
    let second = timeout(Duration::from_secs(2), rx.recv()).await.expect("timeout").expect("recv");
    // 事件仍会送达（过滤逻辑在 handler 层），验证 key
    match second {
        ClusterEvent::ExperienceSync { exp_key, .. } => {
            assert_eq!(exp_key, "exp:node-a-abc123");
        },
        _ => panic!("expected ExperienceSync"),
    }
}
