//! Experience sync module (TODO 9.1.1)
//!
//! 跨节点 experience 同步：
//! - 发送端：本地 store 新增 experience 后，调用 `broadcast_experience` 推送到所有 peer
//! - 接收端：监听 PeerManager 事件，应用 CRDT 合并策略
//!
//! CRDT 策略：LWW-Element-Set + vector clock（参考 sisyphus_types::experience::merge_crdt）
//! - 收到的版本 > 本地版本 → 接受
//! - 收到的 vector clock happened-before 本地 → 丢弃
//! - 并发写入 → LWW (version + node_id tie-break)
//!
//! 接收端必须走 `VectorExperienceBank::apply_remote`（内部即
//! `MfsStore::apply_remote_experience`），而不是 `record()`：后者会用本地
//! node_id bump 时钟，把远程写入伪装成本地新写入，既破坏冲突判定，也让
//! 本地时钟与其他节点失去可比性。

use crate::peer::manager::{ClusterEvent as MgrEvent, PeerManager};
use async_trait::async_trait;
use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_mem::store::ApplyResult;
use sisyphus_types::experience::{
    evaluate_experience_quality, experience_origin_from_key, ExperienceBank, ExperienceQuery,
    ExperienceTuple, FederatedAggregatorConfig,
};
use sisyphus_types::TypesError;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

/// Broadcast 一个 experience 到所有 peer
///
/// payload 为 JSON 序列化的 ExperienceTuple，由接收端解析。
/// `exp_key` 用于去重和过滤（节点 ID 嵌入其中可避免自环）。
pub fn broadcast_experience(
    peer_manager: &Arc<PeerManager>,
    exp: &ExperienceTuple,
    exp_key: impl Into<String>,
) {
    let exp_key = exp_key.into();
    let payload = match serde_json::to_vec(exp) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "broadcast_experience: failed to serialize experience");
            return;
        },
    };
    let vector_clock_json = match serde_json::to_string(&exp.vector_clock) {
        Ok(j) => j,
        Err(e) => {
            tracing::error!(error = %e, "broadcast_experience: failed to serialize vector_clock");
            return;
        },
    };
    peer_manager.broadcast_experience_sync(exp_key, exp.version, vector_clock_json, payload);
}

/// 本地经验广播装饰器：把「本地成功落库」的每次写入推送给所有 peer。
///
/// 注入到 `SisyphusKernel` 的 `Arc<dyn ExperienceBank>`，因此
/// `execute_and_record` 与认知循环 `reflect_phase` 的写入都会自动广播，
/// 无需在两条调用链上分别插入钩子。
///
/// 关键约束：先本地落库、成功后才广播。广播失败只记日志，不影响本地结果——
/// 经验同步是尽力而为的旁路，不应让跨节点故障拖垮本节点的目标执行。
///
/// 不会形成回环：接收端走 [`ExperienceSyncHandler`] → `apply_remote`，
/// 而 `apply_remote` 不经过本装饰器，因此远程应用不会被再次广播。
pub struct PeerBroadcastExperienceBank {
    inner: Arc<VectorExperienceBank>,
    peer_manager: Arc<PeerManager>,
}

impl PeerBroadcastExperienceBank {
    pub fn new(inner: Arc<VectorExperienceBank>, peer_manager: Arc<PeerManager>) -> Self {
        Self {
            inner,
            peer_manager,
        }
    }
}

#[async_trait]
impl ExperienceBank for PeerBroadcastExperienceBank {
    async fn record(&self, exp: ExperienceTuple) -> Result<(), TypesError> {
        // 必须用底层返回的主键广播：它与本地落库的主键完全一致。若在这里
        // 另算一份，对端会把它存成另一行，CRDT 合并也就无从比对。
        let payload = exp.clone();
        let key = self.inner.record_with_id(exp).await?;
        broadcast_experience(&self.peer_manager, &payload, key);
        Ok(())
    }

    async fn query_similar(
        &self,
        query: ExperienceQuery,
    ) -> Result<Vec<ExperienceTuple>, TypesError> {
        self.inner.query_similar(query).await
    }

    async fn export_version_vector(
        &self,
    ) -> Result<sisyphus_types::experience::ExperienceDigest, TypesError> {
        // ADR-010：装饰者转发「我有什么」导出，语义与底层一致。
        self.inner.export_version_vector().await
    }
}

/// ExperienceSyncHandler：订阅 PeerManager 的**入站**事件，将远程 experience 写入本地 bank
///
/// 订阅入站通道而非本地广播通道，这同时解决了两个问题：远程经验能到达（本地
/// 通道里根本没有它们），以及远端事件不会被拨号器再转发回集群形成回弹。
pub struct ExperienceSyncHandler {
    experience_bank: Arc<VectorExperienceBank>,
    peer_manager: Arc<PeerManager>,
    /// 本节点 ID，用于过滤自广播
    self_node_id: String,
    /// ADR-010：入站通道 Lagged 时广播的「请求补齐」信号（ADR-011）。
    ///
    /// `None` 表示不启用（例如测试里只关心合并路径）。拨号器侧订阅它，
    /// 收到后向自己的 peer 重发一次 `SyncRequest`。
    catchup_tx: Option<broadcast::Sender<()>>,
    /// 联邦经验聚合策略（v0.4.0 Phase A）。`None` = 不过滤，保持既有行为。
    federated: Option<FederatedAggregatorConfig>,
}

impl ExperienceSyncHandler {
    /// 创建新的 handler
    pub fn new(
        experience_bank: Arc<VectorExperienceBank>,
        peer_manager: Arc<PeerManager>,
        self_node_id: impl Into<String>,
    ) -> Self {
        Self {
            experience_bank,
            peer_manager,
            self_node_id: self_node_id.into(),
            catchup_tx: None,
            federated: None,
        }
    }

    /// 启用联邦经验聚合（v0.4.0 Phase A）。
    ///
    /// 开启后，入站经验在进入 CRDT 合并之前先按
    /// [`evaluate_experience_quality`] 打分，低于 `min_quality_score` 的经验
    /// 被判定为噪声并丢弃（记 `warn` 日志，不静默）。
    ///
    /// 默认**不启用**（`federated = None`）：过滤会改变收敛结果集，既有反熵/
    /// 混沌收敛用例与「经验全量同步」语义都建立在不过滤之上。开关式接入保证
    /// 单机与既有集群部署行为不变。
    pub fn with_federated_aggregation(mut self, cfg: FederatedAggregatorConfig) -> Self {
        self.federated = Some(cfg);
        self
    }

    /// 开启 Lagged 补齐：入站事件被丢弃时通知拨号器重新拉取差量。
    ///
    /// 传入的 sender 与拨号器侧的 receiver 配对。刻意做成构造期的一次性装配
    /// （而非运行期可调），避免出现「拨号器已订阅但 handler 还没装上信号」的
    /// 窗口——那种窗口内的 Lagged 会被静默吞掉，且难以复现。
    pub fn with_catchup_signal(mut self, catchup_tx: broadcast::Sender<()>) -> Self {
        self.catchup_tx = Some(catchup_tx);
        self
    }

    /// 启动事件循环：监听**入站** peer 事件并处理 ExperienceSync
    ///
    /// `shutdown_rx` 收到信号时退出循环。返回 `JoinHandle` 供调用方管理生命周期。
    ///
    /// 订阅的是入站通道（[`PeerManager::subscribe_inbound`]）而非本地广播通道：
    /// 本地写入已经由 `PeerBroadcastExperienceBank` 直接落库，这里再看到一遍只会
    /// 多做一次无用的 CRDT 合并；而远程经验只有走入站通道才会到达，订阅本地通道
    /// 则完全收不到。订阅入站通道同时意味着事件不会经拨号器回流到集群。
    ///
    /// 任务全程持有 `Arc<PeerManager>`：`broadcast::Receiver` 不会让通道保持存活，
    /// 只有 `Sender` 存活才不会收到 `Closed`。此前 `self.peer_manager` 只在调用
    /// `subscribe()` 时借用一下就被丢弃，于是调用方一旦没有别处持有 PeerManager，
    /// 通道立刻关闭、接收端在 `select!` 里静默退出——进程看起来还在跑，跨节点
    /// 经验同步却已经死了。
    pub fn spawn(self, mut shutdown_rx: broadcast::Receiver<()>) -> JoinHandle<()> {
        let peer_manager = self.peer_manager;
        let mut events = peer_manager.subscribe_inbound();
        let experience_bank = self.experience_bank;
        let self_node_id = self.self_node_id;
        let catchup_tx = self.catchup_tx;
        let federated = self.federated;

        tokio::spawn(async move {
            // 持有一个强引用，保证 Sender 不会在本任务存活期间被丢弃。
            let _keep_manager_alive = peer_manager;
            loop {
                tokio::select! {
                                biased;
                                _ = shutdown_rx.recv() => {
                                    tracing::info!("ExperienceSyncHandler: shutdown signal received");
                                    break;
                                }
                                result = events.recv() => {
                                    match result {
                                        Ok(MgrEvent::ExperienceSync {
                                            exp_key,
                                            version,
                                            vector_clock_json,
                                            payload,
                                        }) => {
                // 防御性自过滤：入站通道已保证事件来自远端，正常情况下 origin
                // 不会是本节点。这里仍保留比对，以防某个对端把本节点的事件原样
                // 回送——那种情况下重复应用是幂等的（CRDT 判为 Equal），但白白
                // 走一次写路径。注意不能用后缀包含判断：origin 不是主键的末段，
                // 那会让 "node-a" 连带吞掉 "node-ab" 的经验。
                if experience_origin_from_key(&exp_key) == Some(self_node_id.as_str()) {
                    tracing::trace!(exp_key = %exp_key, "skipping echoed own experience");
                    continue;
                }
                                            Self::handle_remote_sync(
                                                &experience_bank,
                                                Some(_keep_manager_alive.as_ref()),
                                                &exp_key,
                                                version,
                                                &vector_clock_json,
                                                payload,
                                                federated.as_ref(),
                                            ).await;
                                        }
                                        Ok(_) => {
                                            // 其他事件类型不处理
                                        }
                                        Err(broadcast::error::RecvError::Closed) => {
                                            // 任务自己持有 Arc<PeerManager>，正常情况下
                                            // 不可能到这里。真的到了说明拓扑被拆掉了。
                                            tracing::error!("ExperienceSyncHandler: inbound channel closed unexpectedly; stopping");
                                            break;
                                        }
                                        Err(broadcast::error::RecvError::Lagged(n)) => {
                                            // ADR-011：确实丢了事件。仅记日志意味着那些经验
                                            // 永久缺失——重连也救不回来，因为连接没断。
                                            // 这里主动请求差量重放：反熵按摘要算差量，
                                            // 漏掉的行会被重新算出并经同一幂等合并路径入库。
                                            //
                                            // 不做时间节流：节流假定了「一簇 Lagged 的首次触发
                                            // 能覆盖整簇的丢失」，这个假定是错的。补齐请求会在
                                            // 发出时快照一次摘要，而**快照之后**发生的丢失同样
                                            // 需要补齐；按时间窗抑制掉，就再也没人会来补。
                                            //
                                            // 密集信号由拨号侧限流：`catchup_pending` 保证同一时刻
                                            // 只有一个分页会话，密集信号被合并成一次延迟补齐。
                                            // 上界由同步往返耗时决定，而非人为时间窗。
                                            if let Some(tx) = &catchup_tx {
                                                // Err 只意味着当前没有订阅者（拨号器尚未启动或
                                                // 已退出）。这不致命：拨号器建连时会自己发一次
                                                // 全量反熵拉取。
                                                if let Err(send_err) = tx.send(()) {
                                                    tracing::debug!(
                                                        error = %send_err,
                                                        "Lagged catchup signal has no subscriber"
                                                    );
                                                }
                                            }
                                            tracing::warn!(
                                                lagged = n,
                                                catchup_signalled = catchup_tx.is_some(),
                                                "ExperienceSyncHandler: lagged; requested anti-entropy catch-up"
                                            );
                                        }
                                    }
                                }
                            }
            }
            tracing::info!("ExperienceSyncHandler: stopped");
        })
    }

    /// 处理单个远程 ExperienceSync 事件
    ///
    /// `peer_manager` 仅用于 Phase 6.2b 可观测计数（applied/rejected），不影响合并语义。
    async fn handle_remote_sync(
        experience_bank: &Arc<VectorExperienceBank>,
        peer_manager: Option<&crate::peer::PeerManager>,
        exp_key: &str,
        remote_version: u64,
        vector_clock_json: &str,
        payload: Vec<u8>,
        federated: Option<&FederatedAggregatorConfig>,
    ) {
        // 解析 payload
        let remote_exp: ExperienceTuple = match serde_json::from_slice(&payload) {
            Ok(exp) => exp,
            Err(e) => {
                tracing::warn!(error = %e, exp_key = %exp_key, "failed to deserialize remote experience");
                if let Some(pm) = peer_manager {
                    pm.record_experience_sync_rejected_deserialize();
                }
                return;
            },
        };

        // envelope 与 payload 的 version 必须一致，否则说明传输层有 bug。
        // 不一致时以 payload 为准继续合并，但留下告警便于排查。
        if remote_exp.version != remote_version {
            tracing::warn!(
                exp_key = %exp_key,
                envelope_version = remote_version,
                payload_version = remote_exp.version,
                "experience sync version mismatch; using payload value",
            );
        }
        if serde_json::from_str::<std::collections::BTreeMap<String, u64>>(vector_clock_json)
            .is_err()
        {
            tracing::warn!(
                exp_key = %exp_key,
                "experience sync vector_clock envelope is not a clock map",
            );
        }

        // ── 联邦经验聚合（v0.4.0 Phase A）──────────────────────────────
        // 只影响「是否合并」，不改变合并规则：通过门槛的经验仍走下面的
        // CRDT 判定，因此并发冲突语义不受影响。`None` = 不过滤（默认）。
        if let Some(cfg) = federated {
            let quality = evaluate_experience_quality(&remote_exp, cfg);
            if quality < cfg.min_quality_score {
                tracing::warn!(
                    exp_key = %exp_key,
                    quality,
                    min_quality_score = cfg.min_quality_score,
                    outcome_success = remote_exp.outcome_success,
                    deviation_score = remote_exp.deviation_score,
                    "federated aggregation rejected low-quality remote experience",
                );
                if let Some(pm) = peer_manager {
                    pm.record_experience_sync_rejected_quality();
                }
                return;
            }
            tracing::debug!(
                exp_key = %exp_key,
                quality,
                "federated aggregation accepted remote experience",
            );
        }

        // 关键：走 CRDT 合并路径，而不是 record()。
        // record() 会用本地 node_id bump version/vector_clock，把远程经验当成本地
        // 新写入，于是并发冲突退化为「最后到达者获胜」，且本地时钟被污染后
        // 无法再与其他节点正确比较。apply_remote 把 version/vector_clock 原样
        // 交给 merge_crdt 判定（LWW-Element-Set + vector clock）。
        // 采用发送方算出的主键：它编码了来源节点与写入计数，本地按
        // timestamp 重算会得到不同的 id，同一行将对端与本地各存一份。
        match experience_bank.apply_remote(&remote_exp, exp_key).await {
            Ok(ApplyResult::Applied) => {
                tracing::info!(
                    exp_key = %exp_key,
                    version = remote_exp.version,
                    "applied remote experience to local store",
                );
                if let Some(pm) = peer_manager {
                    pm.record_experience_sync_applied();
                }
            },
            Ok(ApplyResult::Kept) => {
                tracing::debug!(
                    exp_key = %exp_key,
                    version = remote_exp.version,
                    "local experience is newer; remote discarded",
                );
            },
            Ok(ApplyResult::Equal) => {
                tracing::trace!(exp_key = %exp_key, "remote experience already present");
            },
            Err(e) => {
                tracing::error!(error = %e, exp_key = %exp_key, "failed to apply remote experience");
                if let Some(pm) = peer_manager {
                    pm.record_experience_sync_rejected_apply();
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::{spawn_static_peers_with_catchup, StaticPeerConfig};
    use std::time::Duration;

    #[test]
    fn test_broadcast_experience_serializes_payload() {
        let mgr = Arc::new(PeerManager::new("node-a", Duration::from_secs(30)));
        let mut rx = mgr.subscribe();
        let exp = ExperienceTuple {
            context_text: "test".to_string(),
            version: 42,
            ..Default::default()
        };

        broadcast_experience(&mgr, &exp, "exp:test:node-b:42");

        let evt = rx.try_recv().expect("should receive event");
        match evt {
            MgrEvent::ExperienceSync {
                exp_key, version, ..
            } => {
                assert_eq!(exp_key, "exp:test:node-b:42");
                assert_eq!(version, 42);
            },
            _ => panic!("expected ExperienceSync"),
        }
    }

    #[test]
    fn test_broadcast_experience_handles_serialize_error_gracefully() {
        // NaN 字段无法被 JSON 序列化（f64::NAN 不是有效 JSON）
        // 但 ExperienceTuple 的所有字段都使用默认序列化，不会产生 NaN
        // 此测试仅确保函数签名不 panic
        let mgr = Arc::new(PeerManager::new("node-a", Duration::from_secs(30)));
        let exp = ExperienceTuple::default();
        broadcast_experience(&mgr, &exp, "exp:test:1");
        // 无需断言 — 仅确保不 panic
    }

    /// ADR-013 遗留项的实证：那条 `no subscriber` 日志并非死代码。
    ///
    /// ADR 原先写「有 peer 任务存活时永远不会触发」，理由是
    /// `spawn_static_peers_with_catchup` 一进来就 `resubscribe` 了一份接收器。
    /// 但那是**有条件**的结论：`resubscribe` 发生在
    /// `for grpc_addr in &config.peers` 循环体内。`peers` 为空时循环体一次都不
    /// 执行，没有任何接收器被创建，`catchup_tx.send(())` 因此返回 `Err(SendError)`
    /// ——那条日志照常触发。
    ///
    /// 因此 `main.rs` 中未配置任何静态 peer 的节点（单机启动、纯被拨号、
    /// 靠服务发现加入的部署）上，这条分支是**活的**。
    ///
    /// 用例走真实装配路径 `spawn_static_peers_with_catchup`（而非手工造一个
    /// `broadcast::channel`）——手工造通道只能验证 tokio 的语义，证明不了生产
    /// 接线。断言同样取自装配返回的发送端，而不是新建的通道。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn catchup_signal_send_fails_when_peer_list_is_empty() {
        let mgr = Arc::new(PeerManager::new("node-a", Duration::from_secs(30)));
        let (catchup_tx, catchup_rx) = tokio::sync::broadcast::channel::<()>(1);

        // 真实装配路径，但 peers 为空：循环体不执行，不会产生任何 resubscribe，
        // `catchup_rx` 随之被装配函数消费掉——这正是「没有任何订阅者」的来源。
        let dial_cfg = StaticPeerConfig {
            peers: vec![],
            self_node_id: "node-a".to_string(),
            self_grpc_addr: "http://node-a:50051".to_string(),
            join_timeout_secs: 5,
            reconnect_initial_delay: Duration::from_millis(10),
            reconnect_max_delay: Duration::from_millis(50),
            experience_bank: None,
        };
        let handles = spawn_static_peers_with_catchup(dial_cfg, Arc::clone(&mgr), Some(catchup_rx));
        assert!(handles.is_empty(), "peers 为空时不应产生任何拨号任务");

        // 这正是 ExperienceSyncHandler 在 Lagged 分支里执行的那一次发送。
        let send_result = catchup_tx.send(());
        assert!(
            send_result.is_err(),
            "peers 为空时没有任何接收器，`catchup_tx.send(())` 本应返回 \
             Err(SendError)，实际却成功了——说明存在预期之外的订阅者"
        );
    }

    // ── 联邦经验聚合（v0.4.0 Phase A）────────────────────────────────
    //
    // 聚合门槛只决定「是否合并」，不改变 CRDT 判定本身。因此这里全部走真实的
    // `handle_remote_sync` + 真实 `VectorExperienceBank`（内存 sqlite），
    // 断言落在**库里到底有没有那一行**，而不是断言某个返回值——
    // 否则过滤分支被误删也能「看起来通过」。

    /// 在内存 sqlite 上建一个真实经验库，与生产路径同一类型。
    async fn make_bank(node_id: &str) -> Arc<VectorExperienceBank> {
        let mut config = sisyphus_types::Config::default();
        config.cluster.node_id = node_id.to_string();
        let store =
            sisyphus_mem::SisyphusStore::new("sqlite::memory:", &config).await.expect("store init");
        let state_store = store.state.clone();
        let vector_store = Arc::new(
            sisyphus_mem::VectorMemoryStore::new(store.pool()).await.expect("vector store init"),
        );
        Arc::new(
            VectorExperienceBank::new(
                state_store,
                vector_store,
                Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
            )
            .await
            .expect("exp bank init"),
        )
    }

    /// 构造一条远端经验，返回 `(主键, 序列化 payload)`。
    fn remote_payload(counter: u64, success: bool, deviation: f64) -> (String, Vec<u8>) {
        let key = format!("exp:1700000000:node-b:{counter}");
        let exp = ExperienceTuple {
            context_text: format!("remote ctx {counter}"),
            context_hash: format!("remote hash {counter}"),
            action_id: "act".into(),
            outcome_success: success,
            deviation_score: deviation,
            cost: counter,
            timestamp: 1_700_000_000 + counter,
            version: counter,
            ..Default::default()
        };
        (key, serde_json::to_vec(&exp).expect("serialize payload"))
    }

    /// 低质量经验（失败 + 高偏差）在开启聚合后被拒绝：库里没有这一行。
    #[tokio::test]
    async fn federated_aggregation_rejects_low_quality_experience() {
        let bank = make_bank("node-a").await;
        let (key, payload) = remote_payload(1, false, 0.9);
        assert!(
            bank.keys().await.expect("keys").is_empty(),
            "前置条件：新建库应为空"
        );

        ExperienceSyncHandler::handle_remote_sync(
            &bank,
            None,
            &key,
            1,
            "{}",
            payload,
            Some(&FederatedAggregatorConfig::default()),
        )
        .await;

        let keys = bank.keys().await.expect("keys");
        assert!(
            keys.is_empty(),
            "低质量经验应在聚合门槛处被丢弃，实际入库: {keys:?}"
        );
    }

    /// 高质量经验（成功 + 低偏差）在开启聚合后照常合并入库。
    #[tokio::test]
    async fn federated_aggregation_accepts_high_quality_experience() {
        let bank = make_bank("node-a").await;
        let (key, payload) = remote_payload(2, true, 0.1);

        ExperienceSyncHandler::handle_remote_sync(
            &bank,
            None,
            &key,
            2,
            "{}",
            payload,
            Some(&FederatedAggregatorConfig::default()),
        )
        .await;

        let keys = bank.keys().await.expect("keys");
        assert_eq!(keys, vec![key], "高质量经验应被合并入库");
    }

    /// 未配置聚合（`None`）时行为与改造前完全一致：同样的低质量经验照样入库。
    ///
    /// 这是向后兼容的硬约束——既有反熵/混沌收敛用例写入的经验多为
    /// `outcome_success = false`，若默认开启过滤，那些用例会变成「不收敛」。
    #[tokio::test]
    async fn federated_none_keeps_unfiltered_behavior() {
        let bank = make_bank("node-a").await;
        let (key, payload) = remote_payload(3, false, 0.9);

        ExperienceSyncHandler::handle_remote_sync(&bank, None, &key, 3, "{}", payload, None).await;

        let keys = bank.keys().await.expect("keys");
        assert_eq!(keys, vec![key], "未开启聚合时不应过滤任何经验（默认关闭）");
    }
}
