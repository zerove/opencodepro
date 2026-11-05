//! ADR-010 混沌：**全连接 + 双侧反复断流**下的最终一致。
//!
//! ## 为什么单独一个测试二进制
//!
//! 单进程内模拟多节点时，各节点的 origin 只能靠 `SISYPHUS_CLUSTER_NODE_ID`
//! 区分——而 `VectorExperienceBank` 在**每次写入**时读取它。环境变量是进程级的，
//! 若与 `cluster_integration.rs` 里那些并行执行的用例共处一个进程，本用例切换
//! 变量的窗口恰好覆盖它们的写入，就会把它们的行记到 `chaos-a` / `chaos-b`
//! 名下：既污染它们的断言，也让本用例的 origin 隔离失效。
//!
//! cargo 为每个 `tests/*.rs` 起**独立进程**，因此「一个文件一条用例」是这里的
//! 隔离手段，而不是风格选择。
//!
//! ## 为什么需要「反复」断流
//!
//! 既有反熵用例都只经历一次断流（或一次分区）。真实故障是反复发生的，而补漏
//! 逻辑里最容易出问题的恰恰是**第二轮之后**的状态残留：上行游标没重置、摘要
//! 缓存没失效、补齐信号被上一轮的会话吸收。只测一轮的用例对这些全部免疫——
//! 它们的终局在第一次重连时就已经正确。
//!
//! ## 为什么必须双向断开
//!
//! `ClusterServiceImpl::stream` 会把**本节点的本地写入**推给所有已建立的连接
//! （含被 dial 的那条）。只断一个方向时，另一个方向的流仍在实时投递，断连
//! 窗口内的写入照样能到达对端——反熵一次都没被用到，测出来的是「半断」。
//! 因此每轮都关掉两侧的 Join 闸门并断开两侧的已建立流，让收敛只能靠重连后
//! 的反熵拉取。

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

/// 全连接 + 双侧反复断流：每轮断连期间两侧各自写入，重连后必须精确收敛。
///
/// 每轮的四步（缺任何一步都会让用例退化成「没测到想测的东西」）：
/// 1. 关两侧 Join 闸门 → 双向断流 → 等两侧都确认断流生效、拨号方都回到重连路径；
/// 2. 在**无连接**窗口内两侧各写 `PER_SIDE` 条（实时广播无处可去，只能靠反熵）；
/// 3. 放行两侧闸门 → 等**两个方向**都收到新的 `SyncRequest`；
/// 4. 下一轮重复，直到 `ROUNDS` 轮全部完成。
///
/// 终局三条断言，任一条单独成立都不足以说明收敛正确：
/// - **无丢失**：两侧载荷集合相同且等于写入并集；
/// - **无重复**：行数恒等于写入次数（接收端重算主键会把同一经验存成两行）；
/// - **内容一致**：(主键, 载荷) 两侧逐条相同，且同一 `context_text` 不重复。
#[tokio::test]
async fn full_mesh_repeated_partitions_converge_exactly() {
    use std::collections::BTreeSet;

    const ROUNDS: u64 = 3;
    const PER_SIDE: u64 = 4;
    const TOTAL: u64 = ROUNDS * PER_SIDE * 2;

    // 各节点的 origin 在**建库时**就要正确：`VectorExperienceBank::new` 会用它
    // 从库里恢复本地序号。guard 必须活到建库结束，且后一个 guard 覆盖前一个时
    // 不能提前释放（否则 drop 会把变量恢复成上上个值）。
    let _ga = unsafe { EnvVarGuard::set("SISYPHUS_CLUSTER_NODE_ID", "chaos-a") };
    let mut node_a = Node::start("chaos-a").await;
    let _gb = unsafe { EnvVarGuard::set("SISYPHUS_CLUSTER_NODE_ID", "chaos-b") };
    let mut node_b = Node::start("chaos-b").await;

    // 全连接：两个方向各建一条拨号。生产部署里每个节点都拨号其余节点；
    // 单方向拓扑缺一条 pull 路径（ADR-010 明确「由 dial 方发起 pull」），
    // 被拨方断连期间的写入会永久停在本地。
    node_a.dial(node_b.grpc_addr.clone());
    node_b.dial(node_a.grpc_addr.clone());
    node_a.gate.set_join_gate(true);
    node_b.gate.set_join_gate(true);

    let connected = wait_until(Duration::from_secs(15), || async {
        node_a.gate.sync_requests_seen() >= 1 && node_b.gate.sync_requests_seen() >= 1
    })
    .await;
    assert!(
        connected,
        "首连未完成（A 收到 {} 次 SyncRequest，B 收到 {} 次），本用例没有覆盖任何东西",
        node_a.gate.sync_requests_seen(),
        node_b.gate.sync_requests_seen()
    );

    let mut expected: BTreeSet<String> = BTreeSet::new();

    for round in 0..ROUNDS {
        // ── 1. 双向断流 ──────────────────────────────────────────────────
        node_a.gate.set_join_gate(false);
        node_b.gate.set_join_gate(false);
        node_a.gate.break_established_stream();
        node_b.gate.break_established_stream();
        let broke_a = node_a.gate.await_stream_break_applied(Duration::from_secs(10)).await;
        let broke_b = node_b.gate.await_stream_break_applied(Duration::from_secs(10)).await;
        assert!(
            broke_a && broke_b,
            "第 {round} 轮断流未双向生效（A={broke_a}，B={broke_b}），\
             写入并未落在「无连接」窗口内，反熵也没被逼成唯一补偿路径"
        );

        let attempts_a = node_a.gate.join_attempts_seen();
        let attempts_b = node_b.gate.join_attempts_seen();
        let rejoining = wait_until(Duration::from_secs(10), || async {
            node_a.gate.join_attempts_seen() > attempts_a
                && node_b.gate.join_attempts_seen() > attempts_b
        })
        .await;
        assert!(
            rejoining,
            "第 {round} 轮拨号方未全部回到重连路径，写入仍可能被实时送达"
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

        // ── 3. 放行重连，确认两个方向都真的拉过差量 ───────────────────────
        //
        // 只等一侧不够：第 2 步写了两侧，若某一侧的拨号器没重建流，那一侧的
        // 写入就没有补偿路径，而单侧计数仍会增长、断言照样通过。
        let syncs_a = node_a.gate.sync_requests_seen();
        let syncs_b = node_b.gate.sync_requests_seen();
        node_a.gate.set_join_gate(true);
        node_b.gate.set_join_gate(true);
        let resynced = wait_until(Duration::from_secs(10), || async {
            node_a.gate.sync_requests_seen() > syncs_a && node_b.gate.sync_requests_seen() > syncs_b
        })
        .await;
        assert!(
            resynced,
            "第 {round} 轮重连后未双向发出反熵请求（A 收 {} 次，B 收 {} 次）",
            node_a.gate.sync_requests_seen(),
            node_b.gate.sync_requests_seen()
        );
    }

    // ── 终局：两侧收敛到并集，且精确到「一条不多、一条不少」──────────────
    let converged = wait_until(Duration::from_secs(20), || async {
        dump_rows(&node_a.bank).await.len() as u64 == TOTAL
            && dump_rows(&node_b.bank).await.len() as u64 == TOTAL
    })
    .await;

    let a_rows = dump_rows(&node_a.bank).await;
    let b_rows = dump_rows(&node_b.bank).await;
    assert!(
        converged,
        "反复分区后未在期限内收敛：A={} 行，B={} 行，应为 {TOTAL} 行",
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
        "A 行数偏大：远端行被按接收方规则重算主键，同一经验存成了两行"
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
