//! ADR-015（反向反熵）混沌：**单侧拨号拓扑**下的分区 → 恢复后收敛。
//!
//! ## 为什么单独一个测试文件
//!
//! 同目录下的其他混沌用例（`chaos_partition_convergence.rs`）共享进程内的
//! 多节点 harness 与 `SISYPHUS_CLUSTER_NODE_ID` origin；本文件新增的用例需要
//! **单侧拨号**（只有 dialer 主动连 acceptor），拓扑不同，放在独立文件里，
//! 借 cargo 单文件 = 单进程 的隔离机制各自拥有进程。
//!
//! ## 为什么测「单侧拨号」
//!
//! ADR-010 的原始协议只允许「dial 方发起 pull」。在单侧拨号拓扑下（A dial B，
//! B 不 dial A），A 断连重连后会向 B 拉齐；而 B 在分区期间写入的本地经验，
//! 只靠 A 的拉取才有回流路径——A 是 dialer，B 是被动方。若只做全连接测试，
//! 这个「被动方断连期间的写入」的回流缺口永远不会被覆盖到最后一层。
//!
//! 反向反熵（ADR-015）让**服务端在每条双向流建立后也主动发起一次
//! `SyncRequest`**，被 dialer 回送差量。于是即使只有 A dial B，重连后的新
//! 双向流也会让 B 主动向 A 拉齐，单侧拓扑下的写入回流重新闭合。
//!
//! ## 断流位置说明
//!
//! 本用例只存在一条 A→B 的流（B 从不 dial A），承载它的 service impl 是 B 的
//! `ClusterServiceImpl`，因此断流信号与 Join 闸门都打在 B 的实例上。

use sisyphus_bridge::peer::{
    spawn_static_peers_with_catchup, ClusterServiceImpl, ExperienceSyncHandler, JoinGateHandle,
    PeerBroadcastExperienceBank, PeerManager, StaticPeerConfig,
};
use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_mem::VectorMemoryStore;
use sisyphus_types::env_guard::EnvVarGuard;
use sisyphus_types::experience::{ExperienceBank, ExperienceTuple};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tonic::transport::Server;

/// 一个带完整生产接线（peer 管理器 + ClusterService + 经验同步处理器 + 广播装饰）
/// 的测试节点。
struct Node {
    id: &'static str,
    manager: Arc<PeerManager>,
    bank: Arc<VectorExperienceBank>,
    broadcast: Arc<PeerBroadcastExperienceBank>,
    grpc_addr: String,
    gate: JoinGateHandle,
    /// 补齐信号发送端。必须随节点持有：一旦全部发送端被丢弃，
    /// `ExperienceSyncHandler` 会在 `select!` 里收到 `Closed` 并立刻退出，
    /// 节点就变成「收得到事件但不处理」。
    _catchup_tx: broadcast::Sender<()>,
    /// 待交给拨号器的补齐信号接收端（每个 peer 任务各持一份）。
    catchup_rx: Option<broadcast::Receiver<()>>,
    _shutdown: broadcast::Sender<()>,
    _server: tokio::task::JoinHandle<()>,
    _dialers: Vec<tokio::task::JoinHandle<()>>,
}

impl Node {
    /// 起一个节点。`gated` 构造的 Join 闸门初始关闭，由用例显式放行。
    async fn start(id: &'static str) -> Self {
        let manager = Arc::new(PeerManager::new(id, Duration::from_secs(30)));
        let bank = build_bank().await;
        let broadcast = Arc::new(PeerBroadcastExperienceBank::new(
            bank.clone(),
            manager.clone(),
        ));

        let (service, gate) = ClusterServiceImpl::gated(manager.clone(), bank.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let grpc_addr = format!("http://{}", listener.local_addr().expect("addr"));
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(
                    sisyphus_bridge_api::cluster_proto::cluster_server::ClusterServer::new(service),
                )
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .expect("cluster server failed");
        });

        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let (catchup_tx, catchup_rx) = broadcast::channel(1);
        ExperienceSyncHandler::new(bank.clone(), manager.clone(), id)
            .with_catchup_signal(catchup_tx.clone())
            .spawn(shutdown_rx);

        Self {
            id,
            manager,
            bank,
            broadcast,
            grpc_addr,
            gate,
            _catchup_tx: catchup_tx,
            catchup_rx: Some(catchup_rx),
            _shutdown: shutdown_tx,
            _server: server,
            _dialers: Vec::new(),
        }
    }

    /// 向 `peer_addr` 建立静态拨号（生产路径：连接后立即发一次反熵拉取）。
    fn dial(&mut self, peer_addr: String) {
        let cfg = StaticPeerConfig {
            peers: vec![peer_addr],
            self_node_id: self.id.to_string(),
            self_grpc_addr: self.grpc_addr.clone(),
            join_timeout_secs: 5,
            reconnect_initial_delay: Duration::from_millis(50),
            reconnect_max_delay: Duration::from_millis(200),
            experience_bank: Some(self.bank.clone()),
        };
        let handles =
            spawn_static_peers_with_catchup(cfg, self.manager.clone(), self.catchup_rx.take());
        self._dialers.extend(handles);
    }

    /// ADR-015：本节点主动发起的反熵拉取次数（服务端角色）。
    fn outbound_sync_requests_seen(&self) -> u64 {
        self.gate.outbound_sync_requests_seen()
    }
}

async fn build_bank() -> Arc<VectorExperienceBank> {
    let cache = sisyphus_types::CacheConfig::default();
    let store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &cache).await.expect("建内存状态库"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("建内存向量库");
    let vectors = Arc::new(VectorMemoryStore::new(pool).await.expect("初始化向量库"));
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

fn experience(timestamp: u64, text: &str) -> ExperienceTuple {
    ExperienceTuple {
        context_text: text.into(),
        context_hash: format!("hash-{timestamp}"),
        action_id: "chaos-partition".into(),
        project_id: Some("chaos-project".into()),
        outcome_success: true,
        deviation_score: 0.0,
        cost: 1,
        timestamp,
        embedding: None,
        visual_embedding: None,
        insight: None,
        version: 0,
        vector_clock: Default::default(),
    }
}

/// 以指定 origin 身份写入一条经验（主键里的 origin 取自环境变量）。
async fn record_as(node: &Node, origin: &str, exp: ExperienceTuple) {
    let _guard = unsafe { EnvVarGuard::set("SISYPHUS_CLUSTER_NODE_ID", origin) };
    node.broadcast.record(exp).await.expect("写入经验应成功");
}

/// 读出一侧当前持有的全部 (主键, context_text)，按主键排序。
async fn dump_rows(bank: &VectorExperienceBank) -> Vec<(String, String)> {
    let mut keys = bank.keys().await.expect("扫描主键");
    keys.sort();
    let rows = bank.load_by_keys(&keys).await.expect("读取载荷");
    rows.into_iter().map(|(k, e)| (k, e.context_text)).collect()
}

async fn wait_until<F, Fut>(timeout: Duration, mut probe: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if probe().await {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// 单侧拨号 + 反复分区：A dial B，B 不 dial A。分区期间双方写经验；
/// A 在断连期间的写入只停在本地，恢复后的反向反熵（服务端 B 主动拉 dialer A）
/// 才有回流路径把它们带回来。A 的写入则经 dialer 侧拉取带回 B。恢复后必须精确收敛。
#[tokio::test]
async fn one_sided_dialer_reverse_pull_converges_after_partition() {
    use std::collections::BTreeSet;

    const ROUNDS: u64 = 3;
    const PER_SIDE: u64 = 4;
    const TOTAL: u64 = ROUNDS * PER_SIDE * 2;

    let _ga = unsafe { EnvVarGuard::set("SISYPHUS_CLUSTER_NODE_ID", "chaos-a") };
    let mut node_a = Node::start("chaos-a").await;
    let _gb = unsafe { EnvVarGuard::set("SISYPHUS_CLUSTER_NODE_ID", "chaos-b") };
    let node_b = Node::start("chaos-b").await;

    // 单侧拨号：只有 A 主动连 B；B 的 ClusterServiceImpl 承载 A 的双向流。
    node_a.dial(node_b.grpc_addr.clone());
    node_b.gate.set_join_gate(true);

    let connected = wait_until(Duration::from_secs(15), || async {
        // B 在流建立后主动发起反向拉取（ADR-015），A 的拨号器拉 B 的写入；
        // 单侧拓扑下只有 B 是 acceptor，因此「B 拉了一次」即首连完成。
        node_b.outbound_sync_requests_seen() >= 1 && node_b.gate.sync_requests_seen() >= 1
    })
    .await;
    assert!(
        connected,
        "首连未完成（B 主动拉了 {} 次 / A 的拨号拉 B 共收到 {} 次），本用例没有覆盖任何东西",
        node_b.outbound_sync_requests_seen(),
        node_b.gate.sync_requests_seen()
    );

    let mut expected: BTreeSet<String> = BTreeSet::new();

    for round in 0..ROUNDS {
        // ── 1. 断流（A→B 唯一流，由 B 的 service impl 承载）───────────────
        node_b.gate.break_established_stream();
        let broke = node_b.gate.await_stream_break_applied(Duration::from_secs(10)).await;
        assert!(
            broke,
            "第 {round} 轮断流未生效，写入并未落在「无连接」窗口内"
        );
        node_b.gate.set_join_gate(false);

        let attempts_b = node_b.gate.join_attempts_seen();
        let rejoining = wait_until(Duration::from_secs(10), || async {
            node_b.gate.join_attempts_seen() > attempts_b
        })
        .await;
        assert!(
            rejoining,
            "第 {round} 轮拨号方未回到重连路径，写入仍可能被实时送达"
        );

        // ── 2. 无连接窗口内双侧写入 ──────────────────────────────────────
        for i in 0..PER_SIDE {
            let text = format!("chaos-r{round}-a-{i}");
            let ts = 1_700_500_000 + round * 100 + i;
            record_as(&node_a, "chaos-a", experience(ts, &text)).await;
            expected.insert(text);
        }
        for i in 0..PER_SIDE {
            let text = format!("chaos-r{round}-b-{i}");
            let ts = 1_700_500_050 + round * 100 + i;
            record_as(&node_b, "chaos-b", experience(ts, &text)).await;
            expected.insert(text);
        }

        // ── 3. 放行重连，确认反向拉取真的发生了 ───────────────────────────
        let outbound_before = node_b.outbound_sync_requests_seen();
        node_b.gate.set_join_gate(true);
        let resynced = wait_until(Duration::from_secs(10), || async {
            node_b.outbound_sync_requests_seen() > outbound_before
        })
        .await;
        assert!(
            resynced,
            "第 {round} 轮重连后未触发反向反熵拉取（B 主动拉了 {} 次）",
            node_b.outbound_sync_requests_seen(),
        );
    }

    // ── 终局：两侧收敛到并集 ────────────────────────────────────────────
    let converged = wait_until(Duration::from_secs(20), || async {
        dump_rows(&node_a.bank).await.len() as u64 == TOTAL
            && dump_rows(&node_b.bank).await.len() as u64 == TOTAL
    })
    .await;

    let a_rows = dump_rows(&node_a.bank).await;
    let b_rows = dump_rows(&node_b.bank).await;
    assert!(
        converged,
        "单侧拓扑反复分区后未在期限内收敛：A={} 行，B={} 行，应为 {TOTAL} 行",
        a_rows.len(),
        b_rows.len()
    );

    let a_texts: BTreeSet<String> = a_rows.iter().map(|(_, t)| t.clone()).collect();
    let b_texts: BTreeSet<String> = b_rows.iter().map(|(_, t)| t.clone()).collect();
    let a_missing: Vec<&String> = expected.difference(&a_texts).take(10).collect();
    let b_missing: Vec<&String> = expected.difference(&b_texts).take(10).collect();
    assert!(
        a_missing.is_empty() && b_missing.is_empty(),
        "分区期间的写入未被补齐（丢失永久化）。A 缺 {a_missing:?}，B 缺 {b_missing:?}"
    );

    assert_eq!(
        a_rows.len() as u64,
        TOTAL,
        "A 行数偏大：远端行被按接收方规则重算主键"
    );
    assert_eq!(
        a_texts.len() as u64,
        TOTAL,
        "A 存在同 context_text 的多行：内容被覆盖"
    );
    assert_eq!(a_texts, expected, "A 的载荷集合与写入并集不一致");
    assert_eq!(b_texts, expected, "B 的载荷集合与写入并集不一致");
    assert_eq!(
        a_rows, b_rows,
        "两侧 (主键, 载荷) 必须逐条相同：主键不一致意味着接收端没有沿用发送方主键"
    );
}
