//! 静态 peer fallback dialer (TODO 8.3.1 Phase B)
//!
//! 当 mDNS 在 K8s 等多播受限环境下不可用时，使用 `[cluster].peers` 配置
//! 的静态 peer 列表主动 dial 每个 peer 的 gRPC 地址，调用 `Join` 握手，
//! 并保持 `Stream` 双向流连接。
//!
//! 设计要点：
//! - 每个 peer 一个连接，独立的 tokio task（不互相阻塞）
//! - 重连：连接断开后指数退避重试（避免 K8s pod restart 期间风暴）
//! - `Join` 后订阅本地 manager 事件，转发到对端的 `Stream`
//! - 对端推送的事件 → 本地 manager

use crate::peer::manager::PeerManager;
// proto ↔ Rust 类型的转换统一放在 service.rs。
// 此前这里曾镜像一份独立副本，两边各自演化：service.rs 修好入站处理后，
// 拨号器（生产路径走的正是这里）仍在静默丢弃 ExperienceSync，
// 导致跨节点经验同步从未真正打通。单一实现消除这类漂移。
use crate::peer::anti_entropy;
use crate::peer::service::{apply_mgr_event, is_page_end, mgr_event_to_proto, proto_to_mgr_event};
use sisyphus_bridge_api::cluster_proto::cluster_client::ClusterClient;
use sisyphus_bridge_api::cluster_proto::{ClusterEvent as ProtoEvent, JoinRequest};
use sisyphus_mem::experience_bank::VectorExperienceBank;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use tonic::transport::{Channel, Endpoint};

/// 静态 peer dial 配置
#[derive(Clone)]
pub struct StaticPeerConfig {
    /// 静态 peer 列表（gRPC 地址，如 `http://node-1:50051`）
    pub peers: Vec<String>,
    /// 本节点 ID（用于 JoinRequest）
    pub self_node_id: String,
    /// 本节点 gRPC 地址（告知对端如何反向连接）
    pub self_grpc_addr: String,
    /// Join 超时（秒）
    pub join_timeout_secs: u64,
    /// 重连初始延迟（失败后指数退避）
    pub reconnect_initial_delay: Duration,
    /// 重连最大延迟（避免无限增长）
    pub reconnect_max_delay: Duration,
    /// 本节点经验库（ADR-010 反熵：连接建立后据此发 `SyncRequest`）。
    ///
    /// `None` 表示不启用反熵，退化为只依赖实时广播——与反熵落地前一致。
    pub experience_bank: Option<Arc<VectorExperienceBank>>,
}

impl StaticPeerConfig {
    /// 从 sisyphus_types::Config 构造（cluster section）
    pub fn from_types(
        cluster_config: &sisyphus_types::ClusterConfig,
        self_node_id: String,
        self_grpc_addr: String,
    ) -> Self {
        Self {
            peers: cluster_config.peers.clone(),
            self_node_id,
            self_grpc_addr,
            join_timeout_secs: cluster_config.join_timeout_secs,
            reconnect_initial_delay: Duration::from_secs(1),
            reconnect_max_delay: Duration::from_secs(60),
            experience_bank: None,
        }
    }
}

/// 启动静态 peer dialer：对每个 peer 启动一个后台 task。
///
/// 每个 task 流程：
/// 1. 指数退避重连循环
/// 2. 建立 gRPC channel + Join 握手
/// 3. 双向 Stream：本地 manager 事件 → outbound；对端事件 → manager
/// 4. 流断开 → 回到重连循环
///
/// 返回 tokio::JoinHandle 列表（每个 peer 一个）。
pub fn spawn_static_peers(
    config: StaticPeerConfig,
    manager: Arc<PeerManager>,
) -> Vec<tokio::task::JoinHandle<()>> {
    spawn_static_peers_with_catchup(config, manager, None)
}

/// 同 [`spawn_static_peers`]，但订阅「Lagged 补齐」信号。
///
/// ADR-011：入站通道 Lagged 意味着确实丢了事件（而非连接断开），重连无法
/// 补救。收到信号后向本 peer 重发一次 `SyncRequest`，由反熵按摘要算出差量
/// 并经幂等合并路径补齐。`None` 表示不订阅（仅依赖建连时的首次反熵拉取）。
pub fn spawn_static_peers_with_catchup(
    config: StaticPeerConfig,
    manager: Arc<PeerManager>,
    catchup_rx: Option<broadcast::Receiver<()>>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles = Vec::with_capacity(config.peers.len());
    for grpc_addr in &config.peers {
        let cfg = config.clone();
        let mgr = Arc::clone(&manager);
        let addr = grpc_addr.clone();
        let rx = catchup_rx.as_ref().map(broadcast::Receiver::resubscribe);
        let handle = tokio::spawn(async move {
            run_dial_loop(cfg, mgr, addr, rx).await;
        });
        handles.push(handle);
    }
    handles
}

/// 单个 peer 的 dial 循环（指数退避重连）
// adg:allow async function called without await
// 误报：本函数由 spawn_static_peers 里的 tokio::spawn 驱动，
// 并在 run_dial_loop 的调用点 await 其结果，不存在未等待的竞态。
async fn run_dial_loop(
    config: StaticPeerConfig,
    manager: Arc<PeerManager>,
    grpc_addr: String,
    catchup_rx: Option<broadcast::Receiver<()>>,
) {
    let mut backoff = config.reconnect_initial_delay;
    loop {
        match connect_and_join(&config, &grpc_addr).await {
            Ok(channel) => {
                // 连接成功，重置 backoff
                backoff = config.reconnect_initial_delay;
                if let Err(e) = run_stream(
                    &manager,
                    channel,
                    config.experience_bank.as_ref(),
                    catchup_rx.as_ref().map(broadcast::Receiver::resubscribe),
                )
                .await
                {
                    tracing::warn!(
                        grpc_addr = %grpc_addr,
                        error = %e,
                        "static peer stream ended, will reconnect"
                    );
                }
            },
            Err(e) => {
                tracing::warn!(
                    grpc_addr = %grpc_addr,
                    error = %e,
                    backoff_secs = backoff.as_secs(),
                    "static peer connect failed, retrying after backoff"
                );
            },
        }
        tokio::time::sleep(backoff).await;
        // 指数退避（×2，封顶到 max_delay）
        backoff = std::cmp::min(backoff * 2, config.reconnect_max_delay);
    }
}

/// 建立 gRPC channel + Join 握手
async fn connect_and_join(config: &StaticPeerConfig, grpc_addr: &str) -> Result<Channel, String> {
    let endpoint = Endpoint::try_from(grpc_addr.to_string())
        .map_err(|e| format!("invalid endpoint: {e}"))?
        .connect_timeout(Duration::from_secs(config.join_timeout_secs))
        .timeout(Duration::from_secs(config.join_timeout_secs * 2));

    // adg:allow connections must be released after use
    // 误报：tonic 的 Channel 是可克隆的多路复用客户端句柄，
    // 底层连接由 channel 内部池化管理；这里刻意保留原句柄返回给 run_stream，
    // clone 仅用于 Join 客户端，不存在未释放的连接。
    let channel = endpoint.connect().await.map_err(|e| format!("connect: {e}"))?;

    let mut client = ClusterClient::new(channel.clone());

    let response = client
        .join(JoinRequest {
            node_id: config.self_node_id.clone(),
            grpc_addr: config.self_grpc_addr.clone(),
            cluster_name: "default".into(),
        })
        .await
        .map_err(|e| format!("join: {e}"))?
        .into_inner();

    tracing::info!(
        peer_count = response.peers.len(),
        leader_id = %response.leader_id,
        "static peer Join successful"
    );

    Ok(channel)
}

/// 双向 Stream：本地事件 → outbound，对端事件 → manager
///
/// ADR-010：连接建立后发起一次反熵拉取。经验广播是即发即失的——流建好之前
/// 的广播永久丢失，断连期间的写入不会补回，新节点对既有经验一无所知。
/// 这一处同时覆盖「新节点加入」与「重连补偿」两个场景。
async fn run_stream(
    manager: &PeerManager,
    channel: Channel,
    experience_bank: Option<&Arc<VectorExperienceBank>>,
    catchup_rx: Option<broadcast::Receiver<()>>,
) -> Result<(), String> {
    let mut client = ClusterClient::new(channel);

    // outbound: 本地 manager 事件 → proto
    let mut sub = manager.subscribe();
    let (out_tx, out_rx) = mpsc::unbounded_channel::<ProtoEvent>();
    // 克隆必须在 `move` 进广播 task **之前**完成：task 拿走原 `out_tx`，
    // 反熵请求用自己的那一份。
    let sync_tx = out_tx.clone();
    let outbound_task = tokio::spawn(async move {
        loop {
            match sub.recv().await {
                Ok(evt) => {
                    if let Some(proto) = mgr_event_to_proto(evt) {
                        if out_tx.send(proto).is_err() {
                            break;
                        }
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let outbound: UnboundedReceiverStream<ProtoEvent> = UnboundedReceiverStream::new(out_rx);
    let response =
        client.stream(tonic::Request::new(outbound)).await.map_err(|e| format!("stream: {e}"))?;
    let mut inbound = response.into_inner();

    // ADR-010：流已建立，发起一次反熵拉取补齐历史差量。
    // 失败只记日志——反熵是旁路，不应因为它拖垮实时广播与本地写入。
    let mut paging = experience_bank.map(|bank| PagingCursor::new(bank.clone()));
    if let Some(bank) = experience_bank {
        if let Err(e) = anti_entropy::request_sync(bank, &sync_tx, "").await {
            tracing::warn!(error = %e, "anti-entropy: initial sync request failed");
            paging = None;
        }
    }

    let mut catchup_pending = false;

    // 声明为 mut：`select!` 每一轮都以 `&mut` 借用这个 receiver，
    // 跨轮次的接收位置必须保留。
    let mut catchup_rx = catchup_rx;

    // inbound: 对端 proto 事件 → manager
    loop {
        tokio::select! {
            // ADR-011：入站 Lagged 表示确实丢了事件，重连救不回来。向本 peer
            // 重发一次 SyncRequest，反熵按摘要算出差量并经幂等合并补齐。
            // 发送失败只记日志——与初始反熵拉取同等对待，不拖垮主循环。
            _ = recv_catchup(catchup_rx.as_mut()) => {
                match paging {
                    // 已有分页会话在途：不并发开第二个。两个都没有 session ID
                    // 的分页会话一旦交错，服务端下发的 `SyncPageEnd` 就无法归属，
                    // 会把游标推进到错误的偏移。挂起它，等当前会话收尾再从 0 重来。
                    Some(_) => {
                        tracing::warn!("anti-entropy: catch-up deferred until paging finishes");
                        catchup_pending = true;
                    },
                    None => {
                        // 发送失败只记日志——与初始反熵拉取同等对待，不拖垮主循环。
                        if let Some(bank) = experience_bank {
                            if let Err(e) = anti_entropy::request_sync(bank, &sync_tx, "").await {
                                tracing::warn!(error = %e, "anti-entropy: lagged catch-up request failed");
                            } else {
                                paging = Some(PagingCursor::new(bank.clone()));
                                tracing::info!("anti-entropy: lagged catch-up requested");
                            }
                        }
                    },
                }
            },
            result = inbound.next() => {
                // 流已结束：对端关闭了连接。按重连处理，与 Err 分支同样处置。
                let Some(result) = result else {
                    outbound_task.abort();
                    return Err("inbound stream ended".to_string());
                };
                match result {
                    Ok(proto_evt) => {
                        // 反熵应答与实时广播都是 ExperienceSync，接收侧无法从事件本身
                        // 分辨。因此翻页完全由服务端下发的 `SyncPageEnd{ more }` 控制，
                        // 而不是本地计数——否则实时广播会被误计入页内，使游标偏移错乱。
                        if let Some(cursor) = paging.as_mut() {
                            if let Some(marker) = cursor.absorb_page_end(&proto_evt) {
                                if marker.more {
                                    // 先把游标推进到本轮的 next_cursor，再发下一页请求。
                                    // 顺序不能颠倒：用旧游标请求会拿到**同一页**，
                                    // 形成死循环且永不前进。
                                    let next_cursor = marker.next_cursor.clone();
                                    cursor.set_cursor(next_cursor.clone());
                                    match anti_entropy::request_sync(
                                        cursor.bank(),
                                        &sync_tx,
                                        cursor.cursor(),
                                    )
                                    .await
                                    {
                                        Ok(_) => {},
                                        Err(e) => {
                                            tracing::warn!(
                                                error = %e,
                                                cursor = %next_cursor,
                                                "anti-entropy: paging request failed; stopping paging"
                                            );
                                            paging = None;
                                            resume_pending_catchup(
                                                &mut catchup_pending,
                                                experience_bank,
                                                &mut paging,
                                                &sync_tx,
                                            )
                                            .await;
                                        },
                                    }
                                } else {
                                    tracing::info!(
                                        synced = marker.delivered,
                                        next_cursor = %marker.next_cursor,
                                        "anti-entropy: sync completed"
                                    );
                                    paging = None;
                                    resume_pending_catchup(
                                        &mut catchup_pending,
                                        experience_bank,
                                        &mut paging,
                                        &sync_tx,
                                    )
                                    .await;
                                }
                            }
                        }

                        // `SyncPageEnd` 是纯控制标记，不进入 manager 事件总线。
                        if !is_page_end(&proto_evt) {
                            if let Some(mgr_evt) = proto_to_mgr_event(proto_evt) {
                                apply_mgr_event(manager, mgr_evt);
                            }
                        }
                    },
                    Err(status) => {
                        outbound_task.abort();
                        return Err(format!("inbound stream error: {status}"));
                    },
                }
            },
        }
    }
}

/// 等待一次补齐信号。
///
/// 刻意做成每次调用重建的 future 而非常驻 future：`recv()` 一旦返回，
/// 常驻 future 就已完成；在 `select!` 里复用已完成 future 会立即反复 ready，
/// 把补齐变成无休止的反熵请求风暴。每次重建也让「Closed」（发送端已释放）
/// 自然退化为永久 pending，不再反复唤醒 `select!`。
async fn recv_catchup(rx: Option<&mut broadcast::Receiver<()>>) {
    let Some(rx) = rx else {
        // 没有信号源：永久 pending，主循环不受影响。
        std::future::pending::<()>().await;
        return;
    };
    match rx.recv().await {
        Ok(()) => {},
        // 信号本身也可能 Lagged。补齐信号容量极小且语义幂等：一次 Lagged
        // 意味着期间已有多次丢事件，漏掉其中任何一次都会留下缺口，
        // 因此把 Lagged 本身当作「需要补齐」处理，而不是丢弃。
        Err(broadcast::error::RecvError::Lagged(n)) => {
            tracing::warn!(
                lagged = n,
                "anti-entropy: catch-up signal lagged; re-syncing anyway"
            );
        },
        // Sender 已释放：`ExperienceSyncHandler` 已停止，不会再有新的丢事件。
        Err(broadcast::error::RecvError::Closed) => {
            std::future::pending::<()>().await;
        },
    }
}

/// 分页会话收尾后兑现被挂起的补齐信号。
///
/// 从空游标重新发起而不是接着当前偏移：挂起期间本地可能又新增了经验，
/// 增量请求按摘要只回差量，从头重来才覆盖完整区间。
///
/// `pending` 只在**请求真的发出去之后**才清位。若发送失败，标志保留为真，
/// 由下一次 `Lagged` 或下次会话收尾时再次尝试；若在此之前就清位，信号会被
/// 静默吞掉，补齐请求再也发不出去——而失败原因（摘要导出错误、通道关闭）
/// 通常是暂时的，重试本就有意义。
async fn resume_pending_catchup(
    pending: &mut bool,
    experience_bank: Option<&Arc<VectorExperienceBank>>,
    paging: &mut Option<PagingCursor>,
    sync_tx: &mpsc::UnboundedSender<ProtoEvent>,
) {
    if !*pending {
        return;
    }
    let Some(bank) = experience_bank else {
        // 没有经验库，补齐本就无从谈起，清位避免每轮空转。
        *pending = false;
        return;
    };
    match anti_entropy::request_sync(bank, sync_tx, "").await {
        Ok(_) => {
            *pending = false;
            *paging = Some(PagingCursor::new(bank.clone()));
            tracing::info!("anti-entropy: deferred catch-up started");
        },
        Err(e) => {
            // 保留标志：等下一次机会重试，而不是把信号吞掉。
            tracing::warn!(
                error = %e,
                "anti-entropy: deferred catch-up request failed; will retry on next trigger"
            );
        },
    }
}

/// 反熵翻页游标（dial 方）。
///
/// 游标只由服务端的 [`SyncPageEnd`] 标记推进：差量页以普通 `ExperienceSync`
/// 事件的形式发送，与实时广播在 wire 上完全相同，本地无法区分二者，
/// 因此不能靠计数推测页边界。
struct PagingCursor {
    bank: Arc<VectorExperienceBank>,
    /// ADR-012：游标是主键而非下标。
    ///
    /// 差量集合每翻一页就缩短一次（本地收到即持有，那些行随即从差量消失，
    /// 差量整体前移），因此「跳过前 N 条」的下标会在缩短后错位，跳过一批
    /// 仍缺失的行。游标记录最后一个已送达的主键，位置由内容决定。
    cursor: String,
}

impl PagingCursor {
    fn new(bank: Arc<VectorExperienceBank>) -> Self {
        Self {
            bank,
            cursor: String::new(),
        }
    }

    fn bank(&self) -> &Arc<VectorExperienceBank> {
        &self.bank
    }

    fn cursor(&self) -> &str {
        &self.cursor
    }

    fn set_cursor(&mut self, cursor: String) {
        self.cursor = cursor;
    }

    /// 识别页尾标记；非标记事件返回 `None`。
    fn absorb_page_end(
        &mut self,
        evt: &ProtoEvent,
    ) -> Option<sisyphus_bridge_api::cluster_proto::SyncPageEnd> {
        match &evt.event {
            Some(sisyphus_bridge_api::cluster_proto::cluster_event::Event::SyncPageEnd(marker)) => {
                Some(marker.clone())
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::manager::{ClusterEvent as MgrEvent, PeerInfo};

    #[test]
    fn test_static_peer_config_from_types() {
        let cluster_config = sisyphus_types::ClusterConfig {
            peers: vec![
                "http://node-1:50051".to_string(),
                "http://node-2:50051".to_string(),
            ],
            join_timeout_secs: 5,
            ..sisyphus_types::ClusterConfig::default()
        };

        let cfg = StaticPeerConfig::from_types(
            &cluster_config,
            "self-node".to_string(),
            "http://localhost:50051".to_string(),
        );

        assert_eq!(cfg.peers.len(), 2);
        assert_eq!(cfg.self_node_id, "self-node");
        assert_eq!(cfg.join_timeout_secs, 5);
    }

    #[test]
    fn test_mgr_event_to_proto_peer_joined_in_fallback() {
        let evt = MgrEvent::PeerJoined(PeerInfo::new("node-x", "addr-x"));
        let proto = mgr_event_to_proto(evt).expect("convert");
        assert!(proto.event.is_some());
    }

    #[test]
    fn test_mgr_event_to_proto_peer_left_in_fallback() {
        let evt = MgrEvent::PeerLeft {
            node_id: "node-x".into(),
            reason: "shutdown".into(),
        };
        let proto = mgr_event_to_proto(evt).expect("convert");
        assert!(proto.event.is_some());
    }

    #[test]
    fn test_proto_to_mgr_event_peer_joined_in_fallback() {
        use sisyphus_bridge_api::cluster_proto::{
            cluster_event::Event, PeerInfo as ProtoPeerInfo, PeerJoined as PJ,
        };
        let proto = ProtoEvent {
            event: Some(Event::PeerJoined(PJ {
                peer: Some(ProtoPeerInfo {
                    node_id: "node-c".into(),
                    grpc_addr: "addr-c:50051".into(),
                    joined_at: 1000,
                }),
            })),
        };
        let mgr = proto_to_mgr_event(proto).expect("convert");
        assert!(matches!(mgr, MgrEvent::PeerJoined(_)));
    }

    /// 拨号器与服务端共用同一套转换函数；入站 ExperienceSync 必须被保留，
    /// 否则跨节点经验同步在这条生产路径上会被静默丢弃。
    #[test]
    fn test_proto_to_mgr_event_keeps_inbound_experience_sync() {
        use sisyphus_bridge_api::cluster_proto::cluster_event::Event;
        use sisyphus_bridge_api::cluster_proto::ExperienceSync as ES;

        let proto = ProtoEvent {
            event: Some(Event::ExperienceSync(ES {
                exp_key: "exp:1:node-x".into(),
                version: 3,
                vector_clock_json: "{}".into(),
                payload: vec![1, 2, 3],
            })),
        };
        let mgr = proto_to_mgr_event(proto).expect("ExperienceSync 不应被丢弃");
        assert!(matches!(mgr, MgrEvent::ExperienceSync { version: 3, .. }));
    }

    /// 建一个空的经验库（只需 `local_digest` 能导出摘要）。
    async fn test_bank() -> std::sync::Arc<sisyphus_mem::experience_bank::VectorExperienceBank> {
        use sisyphus_mem::experience_bank::VectorExperienceBank;
        use sisyphus_mem::{SisyphusStore, VectorMemoryStore};
        use sisyphus_tools_llm::DummyEmbeddingClient;

        let mut config = sisyphus_types::Config::default();
        config.cluster.node_id = "catchup-test".to_string();
        let store = SisyphusStore::new("sqlite::memory:", &config).await.expect("store init");
        let vector_store =
            Arc::new(VectorMemoryStore::new(store.pool()).await.expect("vector store init"));
        Arc::new(
            VectorExperienceBank::new(
                store.state.clone(),
                vector_store,
                Arc::new(DummyEmbeddingClient),
            )
            .await
            .expect("exp bank init"),
        )
    }

    #[test]
    fn test_endpoint_parse_succeeds() {
        // Endpoint::try_from 校验 URL 格式
        let result = Endpoint::try_from("http://127.0.0.1:50051".to_string());
        assert!(result.is_ok());
        // 非法格式应失败
        let bad = Endpoint::try_from("not a url".to_string());
        assert!(bad.is_err());
    }

    /// 补齐信号不得被发送失败吞掉：`pending` 必须保持为真以便重试。
    ///
    /// 修复前 `*pending = false` 在发请求**之前**执行，于是任何一次发送失败
    /// （摘要导出报错、出站通道已关闭）都会把信号静默丢弃，之后再无补齐时机——
    /// 而失败原因通常是暂时的，重试本就有意义。
    #[tokio::test]
    async fn deferred_catchup_survives_failed_send() {
        let bank = test_bank().await;
        let (tx, rx) = mpsc::unbounded_channel::<ProtoEvent>();
        // 关掉接收端：`send` 确定性地返回 Err，模拟通道已关闭。
        drop(rx);

        let mut pending = true;
        let mut paging: Option<PagingCursor> = None;
        resume_pending_catchup(&mut pending, Some(&bank), &mut paging, &tx).await;

        assert!(pending, "发送失败必须保留补齐信号，否则它被永久吞掉");
        assert!(paging.is_none(), "发送失败不应开启新的分页会话");

        // 第二次尝试：通道仍然关闭，标志必须仍为真（可反复重试）。
        resume_pending_catchup(&mut pending, Some(&bank), &mut paging, &tx).await;
        assert!(pending, "重试同样失败时信号仍须保留");

        // 第三次：换成可用的通道，信号应被兑现并开启分页会话。
        let (tx2, mut rx2) = mpsc::unbounded_channel::<ProtoEvent>();
        resume_pending_catchup(&mut pending, Some(&bank), &mut paging, &tx2).await;
        assert!(!pending, "请求发出成功后必须清位");
        assert!(paging.is_some(), "成功后应开启新的分页会话");
        assert!(
            rx2.try_recv().is_ok(),
            "兑现后必须真的往流里写入了 SyncRequest"
        );
    }

    /// 没有经验库时补齐无从谈起，应当清位以免每轮空转。
    #[tokio::test]
    async fn deferred_catchup_clears_without_bank() {
        let (tx, _rx) = mpsc::unbounded_channel::<ProtoEvent>();
        let mut pending = true;
        let mut paging: Option<PagingCursor> = None;
        resume_pending_catchup(&mut pending, None, &mut paging, &tx).await;
        assert!(!pending, "没有经验库时补齐无从谈起，应清位");
    }
}
