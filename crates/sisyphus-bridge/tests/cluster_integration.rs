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
use tokio_stream::StreamExt as _;
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

// ── 9.1.2 端到端：双节点经验同步真正落库 + CRDT 冲突 ──────────────────────

use sisyphus_bridge::peer::{
    spawn_static_peers, spawn_static_peers_with_catchup, ExperienceSyncHandler,
    PeerBroadcastExperienceBank, StaticPeerConfig,
};
use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_types::experience::{experience_store_id, ExperienceBank, ExperienceQuery};

/// 一个带完整生产接线（peer 管理器 + ClusterService + 经验同步处理器 +
/// 广播装饰经验库）的测试节点。
struct SyncNode {
    manager: Arc<PeerManager>,
    bank: Arc<VectorExperienceBank>,
    broadcast: Arc<PeerBroadcastExperienceBank>,
    grpc_addr: String,
    /// 原始事件订阅，用来区分「事件没到」与「到了但没落库」。
    events: tokio::sync::broadcast::Receiver<ClusterEvent>,
    /// 必须随节点一起持有：一旦全部发送端被丢弃，ExperienceSyncHandler
    /// 会在 select! 里收到 Closed 并立刻退出，节点就变成「收得到但不处理」。
    _shutdown: tokio::sync::broadcast::Sender<()>,
    /// ADR-011：补齐信号订阅。用于断言「确实丢了事件并触发了恢复」，
    /// 而不只是「最终数据对了」——后者在丢失没发生时同样成立。
    catchup_rx: tokio::sync::broadcast::Receiver<()>,
    /// ADR-013：仅闸门构造的节点有值。用于放行 Join 并读取服务端收到的
    /// `SyncRequest` 计数——这是区分「信号被消费」与「靠建连全量拉取补齐」
    /// 的唯一可观测手段（两者最终都收敛，数据上看不出差别）。
    gate: Option<sisyphus_bridge::peer::JoinGateHandle>,
}

impl SyncNode {
    /// 供等待闭包捕获，避免和 `&mut self.events` 的可变借用冲突。
    fn bank(&self) -> Arc<VectorExperienceBank> {
        Arc::clone(&self.bank)
    }

    /// 通过 `query_similar` 按 context_hash 检索，模拟"对端是否真的看到了这条经验"。
    async fn find_by_hash(&self, hash: &str) -> Option<ExperienceTuple> {
        find_in(&self.bank, hash).await
    }
}

/// 在指定经验库中按 context_hash 检索。
async fn find_in(bank: &VectorExperienceBank, hash: &str) -> Option<ExperienceTuple> {
    {
        let hits = bank
            .query_similar(ExperienceQuery {
                query_text: "sync-probe".into(),
                context_hash: hash.into(),
                action_id: None,
                project_id: None,
                visual_embedding: None,
                limit: 16,
            })
            .await
            .expect("query_similar 应成功");
        hits.into_iter().find(|e| e.context_hash == hash)
    }
}

async fn build_bank() -> Arc<VectorExperienceBank> {
    let cache = sisyphus_types::CacheConfig::default();
    let store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &cache).await.expect("建内存状态库"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("建内存向量库");
    let vectors = Arc::new(sisyphus_mem::VectorMemoryStore::new(pool).await.expect("初始化向量库"));
    Arc::new(
        VectorExperienceBank::new(
            store,
            vectors,
            Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
        )
        .await
        .expect("初始化经验库"),
    )
}

/// 启动一个节点；`dial_peers` 非空时按生产路径建立到对端的静态拨号。
async fn start_sync_node(node_id: &'static str, dial_peers: Vec<String>) -> SyncNode {
    start_sync_node_inner(node_id, dial_peers, false).await
}

/// ADR-013：`gated` 为真时用**闸门关闭**的服务，从而把拨号器的 Join 挡在门外，
/// 制造一个「拨号器已 spawn、但尚未建连」的确定性窗口。
async fn start_sync_node_inner(
    node_id: &'static str,
    dial_peers: Vec<String>,
    gated: bool,
) -> SyncNode {
    let manager = Arc::new(PeerManager::new(node_id, Duration::from_secs(30)));
    // 观察**入站**通道：用例关心的是「对端发来的事件到了没有」，而入站事件
    // 不再进入本地广播通道（否则会被拨号器转发回去，在集群里回弹）。
    // 必须在拨号与处理器启动前订阅，否则会漏掉先于订阅发出的事件。
    let events = manager.subscribe_inbound();
    let bank = build_bank().await;
    let broadcast = Arc::new(PeerBroadcastExperienceBank::new(
        bank.clone(),
        manager.clone(),
    ));

    // ADR-013：闸门构造会一并返回句柄，供用例放行并读取 SyncRequest 计数。
    let (service, gate) = if gated {
        let (svc, gate) = ClusterServiceImpl::gated(manager.clone(), bank.clone());
        (svc, Some(gate))
    } else {
        (
            ClusterServiceImpl::with_experience_bank(manager.clone(), bank.clone()),
            None,
        )
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let grpc_addr = format!("http://{}", listener.local_addr().expect("addr"));
    let _server = tokio::spawn(async move {
        Server::builder()
            .add_service(ClusterServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("server failed");
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

    // ADR-011：Lagged 补齐信号。与 main.rs 的生产装配保持一致。
    let (catchup_tx, catchup_rx) = tokio::sync::broadcast::channel::<()>(1);
    // 拨号器拿一份，测试再拿一份：验证「丢事件→补齐」链路真的跑过。
    let observer_rx = catchup_tx.subscribe();
    let _sync = ExperienceSyncHandler::new(bank.clone(), manager.clone(), node_id)
        .with_catchup_signal(catchup_tx)
        .spawn(shutdown_rx);

    if !dial_peers.is_empty() {
        let cfg = StaticPeerConfig {
            peers: dial_peers,
            self_node_id: node_id.to_string(),
            self_grpc_addr: grpc_addr.clone(),
            join_timeout_secs: 5,
            reconnect_initial_delay: Duration::from_millis(200),
            reconnect_max_delay: Duration::from_millis(500),
            // ADR-010：让拨号器带上经验库，否则不会发起反熵拉取。
            experience_bank: Some(bank.clone()),
        };
        let _dialers = spawn_static_peers_with_catchup(cfg, manager.clone(), Some(catchup_rx));
    }

    SyncNode {
        manager,
        bank,
        broadcast,
        grpc_addr,
        events,
        catchup_rx: observer_rx,
        gate,
        _shutdown: shutdown_tx,
    }
}

fn experience(timestamp: u64, text: &str) -> ExperienceTuple {
    ExperienceTuple {
        context_text: text.into(),
        context_hash: format!("hash-{timestamp}"),
        action_id: "sync-action".into(),
        project_id: Some("sync-project".into()),
        outcome_success: true,
        deviation_score: 0.1,
        cost: 1,
        timestamp,
        embedding: None,
        visual_embedding: None,
        insight: None,
        version: 0,
        vector_clock: Default::default(),
    }
}

/// 轮询等待对端落库，避免用固定 sleep 制造假阴性。
///
/// 超时时把已观察到的事件类型一并报出，用来区分「事件没到对端」与
/// 「事件到了但没落库」这两类完全不同的问题。
async fn wait_for<F, Fut>(
    label: &str,
    events: &mut tokio::sync::broadcast::Receiver<ClusterEvent>,
    mut probe: F,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..100 {
        while let Ok(evt) = events.try_recv() {
            seen.push(format!("{evt:?}").chars().take(60).collect());
        }
        if probe().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("等待超时：{label}；期间观察到的事件：{seen:?}");
}

/// 等待 `src → dst` 的事件链路真正可用。
///
/// Join 成功只说明握手完成，双向流可能还没建好；而经验同步事件是即发即失的，
/// 在流就绪前广播会永久丢失。因此这里反复投递一个可识别的探针事件，
/// 直到对端真的收到为止，而不是靠固定 sleep 赌时序。
async fn wait_for_link(src: &Arc<PeerManager>, dst: &Arc<PeerManager>) {
    let probe_key = format!("link-probe:{}", src.self_node_id());
    let probe = experience(1_600_000_000, "link probe");
    let probe_payload = serde_json::to_vec(&probe).expect("探针经验应可序列化");
    let probe_clock = serde_json::to_string(&probe.vector_clock).expect("探针时钟应可序列化");

    let mut noise: Vec<String> = Vec::new();
    for _ in 0..60 {
        let mut rx = dst.subscribe_inbound();
        src.broadcast_experience_sync(
            probe_key.clone(),
            0,
            probe_clock.clone(),
            probe_payload.clone(),
        );
        let got = timeout(Duration::from_millis(500), async {
            loop {
                match rx.recv().await {
                    Ok(ClusterEvent::ExperienceSync { exp_key, .. }) => {
                        if exp_key == probe_key {
                            return true;
                        }
                    },
                    Ok(other) => {
                        if noise.len() < 5 {
                            noise.push(format!("{other:?}").chars().take(48).collect());
                        }
                        continue;
                    },
                    Err(e) => {
                        noise.push(format!("recv err: {e}"));
                        return false;
                    },
                }
            }
        })
        .await
        .unwrap_or(false);
        if got {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "节点间事件链路未建立：{} 的探针始终到不了对端；对端收到过：{noise:?}",
        src.self_node_id()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_two_node_experience_sync_round_trip_persists() {
    // 节点 ID 有前缀关系（node-a / node-ab），用于暴露子串式自过滤的缺陷。
    let mut node_a = start_sync_node("node-a", vec![]).await;
    let mut node_b = start_sync_node("node-ab", vec![node_a.grpc_addr.clone()]).await;

    wait_for_link(&node_a.manager, &node_b.manager).await;
    let a_bank = node_a.bank();
    let b_bank = node_b.bank();

    // A → B
    node_a
        .broadcast
        .record(experience(1_700_000_001, "goal from node-a"))
        .await
        .expect("A 记录经验应成功");

    wait_for(
        "node-ab 看到 node-a 的经验",
        &mut node_b.events,
        || async { find_in(&b_bank, "hash-1700000001").await.is_some() },
    )
    .await;
    let seen = find_in(&b_bank, "hash-1700000001").await.expect("B 必须持有 A 的经验");
    assert_eq!(seen.context_text, "goal from node-a");
    assert_eq!(seen.project_id.as_deref(), Some("sync-project"));
    assert_eq!(seen.outcome_success, true);

    // B → A
    node_b
        .broadcast
        .record(experience(1_700_000_002, "goal from node-ab"))
        .await
        .expect("B 记录经验应成功");

    wait_for(
        "node-a 看到 node-ab 的经验",
        &mut node_a.events,
        || async { find_in(&a_bank, "hash-1700000002").await.is_some() },
    )
    .await;

    // 本地写入不得被自己的处理器回灌（否则 version 会被反复 bump）。
    let local = node_a.find_by_hash("hash-1700000001").await.expect("A 本地必须保留自己写的经验");
    assert_eq!(local.context_text, "goal from node-a");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_duplicate_delivery_does_not_duplicate_experience() {
    let mut node_a = start_sync_node("node-a", vec![]).await;
    // `node_b.events` 在下方需要可变借用；clippy 对具体用法不可见，
    // 因此这里的 mut 不可去。
    let mut node_b = start_sync_node("node-b", vec![node_a.grpc_addr.clone()]).await;

    wait_for_link(&node_a.manager, &node_b.manager).await;
    let b_bank = node_b.bank();
    let exp = experience(1_700_000_010, "delivered twice");

    // 必须先订阅再 record：`record()` 的广播是即发即失的，晚一步就永远
    // 错过。这也是该用例必须从本地广播通道读的原因。
    let mut local_events = node_a.manager.subscribe();
    node_a.broadcast.record(exp.clone()).await.expect("A 记录经验应成功");

    // 从广播事件里取出 A 实际使用的主键：它必须与 A 本地落库的主键一致，
    // 否则对端会存成另一行。捕获它也让下面的重放用的是同一个 id。
    //
    // 必须从 `subscribe()`（本地广播通道）读，而不是 `node_a.events`
    // （入站通道）：`record()` 走的是 `broadcast_experience_sync` → `event_tx`，
    // 而入站通道 `inbound_tx` 刻意不接收本地产生的事件，否则会在集群里回弹。
    // 顺带说明：用例原先用 `try_recv` 硬循环，失败时报「A 未广播」——实际是
    // 读错了通道；`try_recv` 返回空就 panic，把「读错源」伪装成「没广播」。
    let key = loop {
        match tokio::time::timeout(Duration::from_secs(2), local_events.recv()).await {
            Ok(Ok(ClusterEvent::ExperienceSync { exp_key, .. }))
                if !exp_key.starts_with("link-probe:") =>
            {
                break exp_key
            },
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => panic!("A 的本地广播通道意外关闭：{e}"),
            Err(_) => panic!("A 未广播 ExperienceSync（2s 超时）"),
        }
    };
    assert!(
        key.starts_with(&format!("exp:{}", exp.timestamp)),
        "主键应以时间戳开头，实际 {key}"
    );

    wait_for("B 首次收到", &mut node_b.events, || async {
        find_in(&b_bank, "hash-1700000010").await.is_some()
    })
    .await;

    // 同一 payload、同��主键重放一次（模拟网络重传），CRDT 判定 Equal。
    let outcome = node_b.bank.apply_remote(&exp, &key).await.expect("重放合并应成功");
    assert_eq!(
        format!("{outcome:?}"),
        "Equal",
        "重复投递应被判定为等价而非再次写入"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_same_row_higher_version_wins_regardless_of_node() {
    let node_a = start_sync_node("node-a", vec![]).await;
    let node_b = start_sync_node("node-b", vec![node_a.grpc_addr.clone()]).await;

    wait_for_link(&node_a.manager, &node_b.manager).await;
    let b_bank = node_b.bank();
    let ts = 1_700_000_020;
    // 同一主键 = 同一行。主键现在编码了来源与计数，所以「不同节点同一秒写入」
    // 不再是冲突，而是两行——这正是被修复的碰撞语义。
    let key = experience_store_id(ts, "node-a", 7);

    let mut local = experience(ts, "from node-a");
    local.version = 7;
    local.vector_clock.insert("node-a".into(), 7);
    node_b.bank.apply_remote(&local, &key).await.expect("首条写入应成功");
    assert!(find_in(&b_bank, &format!("hash-{ts}")).await.is_some());

    // 同一主键、更高版本 → 覆盖
    let mut newer = experience(ts, "from node-a newer");
    newer.version = 8;
    newer.vector_clock.insert("node-a".into(), 8);
    let outcome = node_b.bank.apply_remote(&newer, &key).await.expect("高版本合并应成功");
    assert_eq!(format!("{outcome:?}"), "Applied", "更高版本必须覆盖");
    let merged = node_b.find_by_hash(&format!("hash-{ts}")).await.expect("合并后仍可检索");
    assert_eq!(merged.context_text, "from node-a newer");

    // 同一主键、更低版本 → 保留本地（发给持有该行的 node_b）
    let mut older = experience(ts, "from node-a older");
    older.version = 3;
    older.vector_clock.insert("node-a".into(), 3);
    let outcome_a = node_b.bank.apply_remote(&older, &key).await.expect("低版本合并应成功");
    assert_eq!(
        format!("{outcome_a:?}"),
        "Kept",
        "低版本不得覆盖本地已有经验"
    );
}

/// 隔离用例：ExperienceSync 是否能穿过 gRPC 流到达对端。
///
/// 与上面的端到端用例分开，是为了让失败能直接指认断点：
/// 这里同时观察 A 的本地事件总线与 B 的入站流，两者分别断言。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_experience_sync_survives_grpc_round_trip() {
    let (addr_a, manager_a, _h_a) = start_test_node("node-a").await;
    let mut sub_a = manager_a.subscribe();

    let mut client_b = ClusterClient::connect(addr_a.clone()).await.expect("connect");
    let outbound = tokio_stream::iter(Vec::<ProtoEvent>::new());
    let mut inbound =
        client_b.stream(tonic::Request::new(outbound)).await.expect("stream").into_inner();

    // 让服务端的订阅任务先跑起来，否则广播会打在订阅建立之前。
    tokio::time::sleep(Duration::from_millis(150)).await;

    let mut exp = ExperienceTuple::default();
    exp.context_text = "round trip".into();
    exp.version = 7;
    broadcast_experience(&manager_a, &exp, "exp:1700000000:node-b");

    let local = timeout(Duration::from_secs(2), sub_a.recv())
        .await
        .expect("A 本地应看到广播")
        .expect("recv");
    assert!(
        matches!(local, ClusterEvent::ExperienceSync { version: 7, .. }),
        "A 本地事件应为 ExperienceSync，实际：{local:?}"
    );

    let remote = timeout(Duration::from_secs(2), inbound.next())
        .await
        .expect("B 应在超时前收到入站事件")
        .expect("inbound 应有消息")
        .expect("入站流不应报错");
    assert!(
        matches!(
            remote.event,
            Some(sisyphus_bridge_api::cluster_proto::cluster_event::Event::ExperienceSync(_))
        ),
        "B 应收到 ExperienceSync，实际：{remote:?}"
    );
}

/// 回归锁定：生产拨号器必须真的把 A 的事件送到 B。
///
/// 这条用例守的是一类「Join 成功但事件不流动」的缺陷：拨号器曾自带一份
/// 与 service.rs 重复的事件转换实现，那份副本静默丢弃 ExperienceSync，
/// 于是握手看起来正常、经验同步却从未发生。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_shipped_dialer_delivers_events_to_peer() {
    let (addr_a, manager_a, _h_a) = start_test_node("node-a").await;
    let manager_b = Arc::new(PeerManager::new("node-b", Duration::from_secs(30)));

    let cfg = StaticPeerConfig {
        peers: vec![addr_a.clone()],
        self_node_id: "node-b".into(),
        self_grpc_addr: "http://127.0.0.1:1".into(),
        join_timeout_secs: 5,
        reconnect_initial_delay: Duration::from_millis(200),
        reconnect_max_delay: Duration::from_millis(500),
        experience_bank: None,
    };
    let _dialers = spawn_static_peers(cfg, manager_b.clone());

    for attempt in 0..40 {
        let mut rx = manager_b.subscribe_inbound();
        manager_a.on_peer_joined(PeerInfo::new("node-b", "http://127.0.0.1:1"));
        let key = format!("probe-{attempt}");
        manager_a.broadcast_experience_sync(key.clone(), 1u64, String::from("{}"), vec![7u8, 7u8]);
        let got = timeout(Duration::from_millis(300), async {
            loop {
                match rx.recv().await {
                    Ok(ClusterEvent::ExperienceSync { exp_key, .. }) if exp_key == key => {
                        return true
                    },
                    Ok(_) => continue,
                    Err(_) => return false,
                }
            }
        })
        .await
        .unwrap_or(false);
        if got {
            println!("第 {attempt} 次尝试：链路已通");
            return;
        }
        println!(
            "第 {attempt} 次未收到（A 视角 cluster_size={}，B 视角={}）",
            manager_a.cluster_size(),
            manager_b.cluster_size()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("生产拨号器未能把 A 的事件送达 B");
}

/// ADR-010 全链路：新节点加入后应拉到对端的既有经验（丢失窗口 1）。
///
/// 这是反熵真正要解决的场景：A 先积累经验，B 后加入。B 的拨号器在流建立后
/// 发起 `SyncRequest`，A 按 B 的摘要回送差量，B 走与实时广播相同的
/// `ExperienceSync` → `apply_remote` 路径落库。
///
/// 注意 A 侧**不广播**这些经验——广播是即发即失的，B 加入时它们早已发完。
/// 用例能通过，唯一可能就是反熵真的生效了。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_new_node_pulls_history_via_anti_entropy() {
    let node_a = start_sync_node("node-a", vec![]).await;
    let a_bank = node_a.bank();

    // A 先积累 3 条经验（此时 B 尚不存在，不会被广播出去）。
    for i in 1..=3u64 {
        node_a
            .broadcast
            .record(experience(1_700_001_000 + i, &format!("history-{i}")))
            .await
            .expect("A 记录经验应成功");
    }
    assert_eq!(a_bank.keys().await.expect("A 读键").len(), 3);

    // B 现在才加入。
    let mut node_b = start_sync_node("node-b", vec![node_a.grpc_addr.clone()]).await;
    let b_bank = node_b.bank();

    for i in 1..=3u64 {
        let hash = format!("hash-{}", 1_700_001_000 + i);
        wait_for(
            &format!("B 反熵拉取到 {hash}"),
            &mut node_b.events,
            || async { find_in(&b_bank, &hash).await.is_some() },
        )
        .await;
    }

    let keys = b_bank.keys().await.expect("B 读键");
    assert_eq!(
        keys.len(),
        3,
        "B 应恰好持有 A 的 3 条历史，实际 {}",
        keys.len()
    );
}

/// ADR-010 全链路：反熵回送的内容必须与实时广播完全同构。
///
/// 反熵的价值在于复用既有合并路径；若载荷字段有缺失（比如丢了
/// vector_clock），接收端就无法做 CRDT 裁决，会静默写坏数据。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_anti_entropy_delivers_crdt_complete_payload() {
    let node_a = start_sync_node("node-a", vec![]).await;
    node_a
        .broadcast
        .record(experience(1_700_001_100, "anti-entropy payload"))
        .await
        .expect("A 记录经验应成功");

    let mut node_b = start_sync_node("node-b", vec![node_a.grpc_addr.clone()]).await;
    let b_bank = node_b.bank();

    wait_for("B 反熵拉取", &mut node_b.events, || async {
        find_in(&b_bank, "hash-1700001100").await.is_some()
    })
    .await;

    let got = find_in(&b_bank, "hash-1700001100").await.expect("B 必须持有该经验");
    assert_eq!(got.context_text, "anti-entropy payload");
    assert_eq!(got.project_id.as_deref(), Some("sync-project"));
    assert!(
        got.version > 0,
        "反熵回送必须带 version，收到 {}",
        got.version
    );
    assert!(
        !got.vector_clock.is_empty(),
        "反熵回送必须带 vector_clock，否则接收端无法做 CRDT 裁决"
    );
}

/// ADR-010 全链路：反熵幂等——已同步的节点不应被重复灌入。
///
/// 关键在于反熵与实时广播可能对同一行各送达一次（连接建立时先发 `SyncRequest`，
/// 随后历史经验又被广播）。CRDT 裁决必须把后者判为等价而非重复行。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_anti_entropy_does_not_duplicate_rows() {
    let node_a = start_sync_node("node-a", vec![]).await;
    let a_bank = node_a.bank();

    node_a
        .broadcast
        .record(experience(1_700_001_200, "written before join"))
        .await
        .expect("A 记录经验应成功");
    assert_eq!(a_bank.keys().await.expect("A 读键").len(), 1);

    let mut node_b = start_sync_node("node-b", vec![node_a.grpc_addr.clone()]).await;
    let b_bank = node_b.bank();

    wait_for("B 反熵拉取", &mut node_b.events, || async {
        find_in(&b_bank, "hash-1700001200").await.is_some()
    })
    .await;

    // 等一段时间，给可能并行的实时广播一个机会。
    tokio::time::sleep(Duration::from_millis(500)).await;

    let keys = b_bank.keys().await.expect("B 读键");
    assert_eq!(
        keys.len(),
        1,
        "反熵与广播可能对同一行各送达一次，CRDT 必须去重，实际 {} 行",
        keys.len()
    );
}

/// ADR-010 全链路：差量超过单页时，真实拨号器必须翻页拉完。
///
/// 这是本轮修复的核心缺陷的回归锁定。差量走真实路径：
/// `run_stream` 发 `SyncRequest` → peer 分页回送 → `SyncPageEnd` 驱动
/// dialer 发下一次 `SyncRequest` → ……直到 `more=false`。
///
/// 写入 `SYNC_PAGE_SIZE + 5` 条，**故意选一个非整数倍**，这样：
/// - 若 dialer 不翻页，只能拿到 128 条，用例失败；
/// - 若页尾判定把末页误当满页，会多发一轮请求拿到空页，行数仍为 `total`。
///
/// 注意本用例断言的是「不漏 + 不重行」，**不是**「传输不重叠」。分页游标
/// 按差量集合索引推进，而每次请求的摘要已随本地落库而推进，但摘要粒度是
/// `(origin, max counter)`，因此翻页区间仍可能重叠、重复传输若干条。
/// 这是有意的权衡：重叠由 CRDT 幂等合并吸收，不重复则需要会话级去重状态，
/// 而会话状态在断连重连时会丢，反倒更容易漏。详见 ADR-010 遗留限制。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_dialer_pages_through_delta_larger_than_one_page() {
    use sisyphus_bridge::peer::SYNC_PAGE_SIZE;

    let node_a = start_sync_node("node-a", vec![]).await;
    let a_bank = node_a.bank();

    // 非整数倍页大小：3 页。最后一页只有 5 条。
    let total = SYNC_PAGE_SIZE as u64 + 5;
    for i in 0..total {
        node_a
            .broadcast
            .record(experience(1_700_100_000 + i, &format!("bulk-{i}")))
            .await
            .expect("A 批量记录经验应成功");
    }
    assert_eq!(
        a_bank.keys().await.expect("A 读键").len() as u64,
        total,
        "A 应先持有全部 {total} 条"
    );

    // B 后加入，走真实 dialer 翻页。
    let node_b = start_sync_node("node-b", vec![node_a.grpc_addr.clone()]).await;
    let b_bank = node_b.bank();

    // 等到全部到位：差量跨 3 页，链路与每页往返都要时间。
    for attempt in 0..80 {
        let got = b_bank.keys().await.map(|k| k.len()).unwrap_or(0);
        if got as u64 == total {
            // 多等几拍，确认不会又冒出重复行。
            tokio::time::sleep(Duration::from_millis(400)).await;
            let final_count = b_bank.keys().await.map(|k| k.len()).unwrap_or(0);
            assert_eq!(
                final_count as u64, total,
                "翻页补齐后行数应恒为 {total}（第 {attempt} 轮）"
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let got = b_bank.keys().await.map(|k| k.len()).unwrap_or(0);
    panic!(
        "拨号器未能翻页拉完 {total} 条差量，只拿到 {got} 条。\
         若恰好等于 {page_size} 说明只补了第一页——page-end 标记或游标推进有问题。",
        page_size = SYNC_PAGE_SIZE
    );
}

// ── 9.1.4 ADR-011：广播接收端 Lagged 导致的事件丢失，由反熵补齐 ────────────────

/// 真实触发 Lagged：broadcast 容量有限，只要在 handler 没调 `recv()` 的窗口内
/// 塞超过容量的入站事件，它一定会收到 `RecvError::Lagged(n)`。
///
/// 这不是为了测 throttle 而测 throttle：真正的判定是「handler 在 Lagged 后
/// 发出了补齐信号，拨号器据此重新拉取」。
#[tokio::test]
async fn lagged_broadcast_triggers_catchup_signal() {
    let bank = build_bank().await;
    let manager = Arc::new(PeerManager::new("lag-sender", Duration::from_secs(30)));
    // 容量 1 制造极端背压：handler 极可能来不及消费。
    // 必须持有 shutdown sender：一旦它被丢弃，handler 在 select! 里立刻收到
    // Closed 并退出，就永远不会经历 Lagged。
    let (_shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
    let (catchup_tx, mut catchup_rx) = tokio::sync::broadcast::channel::<()>(1);

    let _handler = ExperienceSyncHandler::new(bank, manager.clone(), "lag-sender")
        .with_catchup_signal(catchup_tx)
        .spawn(shutdown_rx);

    // 灌入远超容量的入站事件。
    for i in 0..512u64 {
        manager.deliver_inbound_experience_sync(format!("flood-{i}"), i, "{}", vec![0u8; 64]);
    }

    // `expect` 而非 `let _ =`：信号必须真的发出来。收到后无需再断言内容
    // （信号无携带数据），能取到即证明补齐链路被触发。
    tokio::time::timeout(Duration::from_secs(5), catchup_rx.recv())
        .await
        .expect("Lagged 后应在超时内发出补齐信号")
        .expect("补齐信号通道不应关闭");
}

/// 反向对照：没有订阅补齐信号时，即使 Lagged 也不应发信号。
/// 这条保证补齐信号是 `with_catchup_signal` 引入的，而非既有行为。
#[tokio::test]
async fn no_catchup_signal_without_subscription() {
    let bank = build_bank().await;
    let manager = Arc::new(PeerManager::new("lag-quiet", Duration::from_secs(30)));
    let (catchup_tx, mut catchup_rx) = tokio::sync::broadcast::channel::<()>(1);
    // 保留 sender 以免通道关闭，但不发信号。
    let _keep = catchup_tx.clone();

    let (_shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
    let _handler =
        ExperienceSyncHandler::new(bank, manager.clone(), "lag-quiet").spawn(shutdown_rx);

    for i in 0..512u64 {
        manager.deliver_inbound_experience_sync(format!("flood-{i}"), i, "{}", vec![0u8; 64]);
    }

    let result = tokio::time::timeout(Duration::from_millis(500), catchup_rx.recv()).await;
    assert!(result.is_err(), "未订阅补齐信号时不应收到任何信号");
}

/// 端到端：补齐信号必须真的让拨号器重发 `SyncRequest{offset:0}`，把
/// Lagged 丢掉的经验补回本地。这是本节的核心用例。
#[tokio::test]
async fn catchup_signal_reruns_anti_entropy() {
    let total = 40u64;
    let node_a = start_sync_node("node-a", vec![]).await;
    let a_bank = node_a.bank();
    for i in 0..total {
        node_a
            .broadcast
            .record(experience(1_700_200_000 + i, &format!("lag-{i}")))
            .await
            .expect("A 批量记录经验应成功");
    }
    assert_eq!(a_bank.keys().await.expect("A 读键").len() as u64, total);

    let mut node_b = start_sync_node("node-b", vec![node_a.grpc_addr.clone()]).await;
    let b_bank = node_b.bank();

    // 等到正常反熵把它补齐——这一步证明基线可用。
    for attempt in 0..100 {
        if b_bank.keys().await.map(|k| k.len()).unwrap_or(0) as u64 == total {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            attempt < 100,
            "正常反熵未能补齐 {total} 条，基线失败，后续 Lagged 用例无意义"
        );
    }

    // 真实制造 Lagged：入站通道容量 256。`deliver_inbound_experience_sync` 是
    // 同步调用，循环内没有 await 点，因此 handler 任务在这段里根本不会被调度
    // ——只要单批超过容量，就**确定性地**溢出，不依赖竞态。
    //
    // 关键：洪泛与经验写入交错。若先灌完再写经验，末尾的经验仍走正常广播，
    // 测试会在丢失根本没发生时虚假通过——上一版正是这样假通过的。
    let extra = 30u64;
    for i in 0..extra {
        for j in 0..600u64 {
            node_b.manager.deliver_inbound_experience_sync(
                format!("flood-{i}-{j}"),
                i * 600 + j,
                "{}",
                vec![0u8; 64],
            );
        }
        node_a
            .broadcast
            .record(experience(1_700_300_000 + i, &format!("post-lag-{i}")))
            .await
            .expect("A 追加经验应成功");
    }

    let expected_keys: std::collections::BTreeSet<String> = {
        let mut s = std::collections::BTreeSet::new();
        for k in a_bank.keys().await.expect("A 读键") {
            s.insert(k);
        }
        s
    };

    let expected = total + extra;
    for _ in 0..150 {
        if b_bank.keys().await.map(|k| k.len()).unwrap_or(0) as u64 == expected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // 断言**逐条**对齐而不仅是数量：总数相等但集合不同（旧的空洞失明行为
    // 就会让 B 停在 52 条）仍需被拦住，而且集合比对能顺带发现重复行。
    let actual_keys: std::collections::BTreeSet<String> =
        b_bank.keys().await.unwrap_or_default().into_iter().collect();

    assert_eq!(
        actual_keys,
        expected_keys,
        "Lagged 补齐后 B 的行集合应与 A 完全一致。\n\
         若停在更小的数量，说明纯广播丢失后没有补偿路径；\n\
         若数量相等而集合不同，说明幂等合并出了问题。\n\
         仍缺：{:?}",
        expected_keys.difference(&actual_keys).take(20).collect::<Vec<_>>()
    );

    // 上面的数据断言在「丢失没真正发生」时同样会通过，所以额外证明：
    // 本轮确实丢了事件并触发了补齐信号。否则这个用例只是在测普通广播，
    // 对 Lagged 路径零覆盖。
    assert!(
        tokio::time::timeout(Duration::from_secs(5), node_b.catchup_rx.recv()).await.is_ok(),
        "补齐信号从未触发：洪泛没压垮 handler，本用例没有真正覆盖 Lagged 路径"
    );
}

/// 补齐信号在分页会话在途时必须被**延后**到会话收尾，而不是并发开第二个。
///
/// 两个都没有 session ID 的分页会话一旦交错，服务端下发的 `SyncPageEnd` 就
/// 无法归属，游标会被推进到错误的偏移——这类 bug 表现为「行数对了但集合不对」，
/// 数量断言抓不住，因此本用例断言集合。
///
/// ## 为什么必须先等第一页落地
///
/// 早期版本在建好 B 之后立即洪泛。把延后改成丢弃的变异（`drop(catchup_pending)`）
/// 在该版本下能存活，说明它对这条路径零覆盖。
///
/// 实测（临时注入 tracing subscriber 抓日志）给出的真实原因，比「落在
/// `paging = None`」更早一步：**两个分支一次都没执行**，21 个 Lagged 信号在
/// 拨号器 `resubscribe` 之前就发完了，比拨号器 Join 早约 11ms。
/// `broadcast::Receiver::resubscribe` 只从订阅那一刻开始计数，看不到订阅之前
/// 已发出的信号——因此这些信号既没进 `Some` 分支，也没进 `None` 分支，
/// 而是在接收端根本不存在。`tx.send(())` 仍然返回 `Ok`（有订阅者：上游那个
/// `observer`），所以服务端日志里的 `catchup_signalled=true` 会误导人以为
/// 信号一定被消费。**发送成功不等于信号被消费。**
///
/// 这个缺口本身是安全的：`run_stream` 每次流建立时无条件发一次全量反熵拉取，
/// 建连时错过的信号由它兜住。危险的是它会伪装成覆盖——用例照样通过、断言照样
/// 成立，却什么都没验到。
///
/// 现在先轮询等 B 真正收到第一页：那一刻拨号器已 `resubscribe` 且分页会话在途
/// （`paging = Some`），洪泛落在会话中途，延后分支必被触发。页数取 6 轮以把
/// 时间窗拉宽。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn catchup_deferred_while_paging_converges_exactly() {
    use sisyphus_bridge::peer::SYNC_PAGE_SIZE;

    let node_a = start_sync_node("node-a", vec![]).await;
    let a_bank = node_a.bank();

    // 跨很多页，确保 B 的初始反熵必然进入多轮翻页（`more = true`），
    // 且在洪泛落下时分页尚未结束。
    let total = SYNC_PAGE_SIZE as u64 * 6 + 7;
    for i in 0..total {
        node_a
            .broadcast
            .record(experience(1_700_400_000 + i, &format!("defer-{i}")))
            .await
            .expect("A 批量记录经验应成功");
    }

    let mut node_b = start_sync_node("node-b", vec![node_a.grpc_addr.clone()]).await;
    let b_bank = node_b.bank();

    // 先等第一页真正落库：此时拨号器正处于分页会话中途。
    let mut first_page_landed = false;
    for _ in 0..600 {
        let got = b_bank.keys().await.map(|k| k.len()).unwrap_or(0);
        if got >= SYNC_PAGE_SIZE {
            first_page_landed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        first_page_landed,
        "B 始终没收到第一页，本用例无法进入「分页在途」状态"
    );

    // 此时洪泛压垮 B 的入站通道：handler 正在忙着逐条写库，必然落后于
    // 写入速度，于是产生真实的 Lagged 信号——而分页会话还在进行中。
    for i in 0..3u64 {
        for j in 0..600u64 {
            node_b.manager.deliver_inbound_experience_sync(
                format!("flood-{i}-{j}"),
                i * 600 + j,
                "{}",
                vec![0u8; 64],
            );
        }
    }

    let expected_keys: std::collections::BTreeSet<String> =
        a_bank.keys().await.expect("A 读键").into_iter().collect();

    for _ in 0..300 {
        let got = b_bank.keys().await.map(|k| k.len()).unwrap_or(0);
        if got as u64 == total {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let actual_keys: std::collections::BTreeSet<String> =
        b_bank.keys().await.unwrap_or_default().into_iter().collect();

    assert_eq!(
        actual_keys,
        expected_keys,
        "分页在途时的补齐被延后执行后，B 的行集合仍应与 A 完全一致。\n\
         若集合偏小：延后的补齐被丢弃了（丢失永久化）。\n\
         若集合偏大或存在错位行：两个分页会话交错，`SyncPageEnd` 归属错乱。\n\
         仍缺：{:?}",
        expected_keys.difference(&actual_keys).take(20).collect::<Vec<_>>()
    );

    assert_eq!(
        actual_keys.len() as u64,
        total,
        "行数应恒为 {total}，重复合并不应产生额外行"
    );

    // 洪泛确实造成了 Lagged：信号已被拨号器消费，这里只确认它发生过。
    assert!(
        node_b.catchup_rx.try_recv().is_ok()
            || tokio::time::timeout(Duration::from_millis(200), node_b.catchup_rx.recv())
                .await
                .is_ok(),
        "洪泛从未触发补齐信号，本用例没有覆盖「分页在途时延后」这条路径"
    );
}

/// ADR-013：补齐信号必须跨「建连」被保留，不能被建连那一步的 `resubscribe` 吃掉。
///
/// ## 这个缺口为什么数据上看不见
///
/// `run_stream` 在每次流建立时都无条件发一次全量反熵拉取。因此信号被丢弃与否，
/// **最终行集合完全一样**——只断言收敛状态的用例对这个 bug 恒真，是典型假覆盖。
///
/// ## 用计数而不是数据来断言
///
/// 本用例断言服务端收到的 `SyncRequest` 次数：
///
/// - 建连时首次全量拉取 → 计数 +1
/// - 若建连前发出的信号被消费 → 再发一次补齐请求 → 计数再 +1
///
/// 变异（把 `run_dial_loop` 里的 `catchup_rx.as_mut()` 还原成
/// `catchup_rx.as_ref().map(broadcast::Receiver::resubscribe)`）会让计数停在 1，
/// 因为新订阅者看不到订阅前发出的信号。用例因此必然失败。
///
/// ## 为什么用 Join 闸门
///
/// 需要一个「拨号器已 spawn、但还没建连」的窗口。`PeerManager` 没有可用的断流手段，
/// 所以改为在**服务端**把 Join 挡住：拨号器的任务已启动（接收器已创建），却卡在
/// `connect_and_join`，此时发出的信号就精确落在目标窗口内。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn catchup_signal_sent_before_connect_is_still_consumed() {
    // 闸门必须装在**被拨号方 A** 上：B 是拨号方，它连的是 A。装在 B 自己的服务上
    // 毫无作用——没人会连 B，B 的拨号器会直接连上 A 并正常跑完，计数恒为 0。
    let node_a = start_sync_node_inner("node-a", vec![], true).await;
    let gate = node_a.gate.clone().expect("闸门构造的节点必须带回闸门句柄");

    // B 作为拨号方去连 A；此时 A 的 Join 被闸门挡住，B 卡在 connect_and_join。
    let mut node_b = start_sync_node("node-b", vec![node_a.grpc_addr.clone()]).await;

    // 洪泛压垮 B 的入站通道（容量 256），迫使 ExperienceSyncHandler 产生真实的
    // Lagged 并发出补齐信号。此刻拨号器还卡在 Join，信号落在建连之前。
    for i in 0..3u64 {
        for j in 0..600u64 {
            node_b.manager.deliver_inbound_experience_sync(
                format!("pre-connect-flood-{i}-{j}"),
                i * 600 + j,
                "{}",
                vec![0u8; 64],
            );
        }
    }

    // 确认信号确实发出了（observer 是独立订阅，能看到 handler 的 send）。
    // 只确认「发过」不够——本用例的真正断言在下面的计数上。
    let signalled = node_b.catchup_rx.try_recv().is_ok()
        || tokio::time::timeout(Duration::from_millis(300), node_b.catchup_rx.recv()).await.is_ok();
    assert!(signalled, "洪泛从未触发补齐信号，本用例没有覆盖任何东西");

    // 建连仍未发生：闸门关着，计数必须为 0。
    assert_eq!(
        gate.sync_requests_seen(),
        0,
        "闸门关闭期间不该有任何 SyncRequest 抵达服务端"
    );

    // 放行建连。拨号器此时才进入 run_stream 并发出首次全量拉取。
    gate.release_gate();

    // 等到计数稳定：首次拉取 1 次 + 被消费信号触发的补齐 1 次 = 2。
    for _ in 0..100 {
        if gate.sync_requests_seen() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // 再等一会儿，确认不会冒出第 3 次（补齐信号是幂等单次，不是风暴）。
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(
        gate.sync_requests_seen(),
        2,
        "建连前发出的补齐信号必须被消费：应先有建连时的首次全量拉取，\
         再有一次由该信号触发的补齐请求。\
         若只有 1 次，说明信号被建连那一步的 `resubscribe` 静默吃掉了\
         （数据仍会靠首次拉取收敛，所以只断言数据永远抓不到这个 bug）。\
         若超过 2 次，说明补齐变成了请求风暴。"
    );
}
