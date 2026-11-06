//! 集群策略权重同步（v0.4.0 Phase B，ADR-017）
//!
//! 每个节点周期性地把本地 [`MetaLearner`] 的策略统计打包成
//! [`NodeStrategySnapshot`]，经 `StateProposal` 集群事件广播；收到对端快照后
//! 按 `generation` 合并到共享的 [`FederatedStrategyWeights`]。
//!
//! ## 为什么用 `StateProposal` 而不是 `Consensus::propose`
//!
//! `ClusterConsensus::propose` 只有在注入了 `PeerProposer` 时才真正发送，
//! 生产装配里 `ClusterConsensus::new("node-1", vec![])` 的 proposer 是 `None`，
//! 于是 `propose()` 只写一行日志。用它会得到一个「代码看着在同步、日志看着在
//! 同步、数据永远不到」的路径。
//!
//! `StateProposal` 是集群事件总线里已打通双向传输的类型：`mgr_event_to_proto`
//! 早已支持出站，Phase B 补上了入站的
//! [`PeerManager::deliver_inbound_state_proposal`]。
//!
//! ## 幂等与传播
//!
//! 合并规则在 `sisyphus_types::strategy_weights`：同节点按 `generation`
//! 严格取新，因此**重复投递、乱序到达都是安全的**。这一点是必要的——
//! `StateProposal` 走的是「每个已建立的双向流各发一次」，节点数一多，
//! 同一份快照会通过多条流重复到达。

use crate::peer::manager::{ClusterEvent as MgrEvent, PeerManager};
use parking_lot::RwLock;
use sisyphus_types::meta_learner::MetaLearner;
use sisyphus_types::strategy_weights::{FederatedStrategyWeights, NodeStrategySnapshot};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

/// 策略权重快照在 `StateProposal.key` 上的标识。
///
/// 带 `/v1` 是因为载荷是 JSON 且字段名即契约：将来若改结构，旧节点必须能
/// **安全忽略**新载荷，而不是把解析失败当成「对端没数据」。key 不匹配即忽略。
pub const STRATEGY_WEIGHTS_KEY: &str = "sisyphus.strategy-weights/v1";

/// 默认发布周期（秒）。
///
/// 60 秒是「策略统计的变化率」与「集群广播开销」的折中：`MetaLearner` 的
/// 统计只在执行结束后变化，秒级同步没有意义，而分钟级足以让 REST 视图与
/// Phase C 的收敛断言在几轮内看到对端数据。
pub const DEFAULT_PUBLISH_INTERVAL_SECS: u64 = 60;

/// 策略权重同步处理器。
pub struct StrategyWeightsHandler {
    peer_manager: Arc<PeerManager>,
    /// 本节点 ID：既用于打快照标签，也用于拒绝自回环。
    self_node_id: String,
    meta_learner: Arc<parking_lot::Mutex<MetaLearner>>,
    shared: Arc<RwLock<FederatedStrategyWeights>>,
    /// 发布周期；`Duration::ZERO` = 只收不发（便于只要聚合视图的场景）。
    publish_interval: Duration,
}

impl StrategyWeightsHandler {
    /// 创建处理器（默认 60 秒发布一次）。
    pub fn new(
        peer_manager: Arc<PeerManager>,
        self_node_id: impl Into<String>,
        meta_learner: Arc<parking_lot::Mutex<MetaLearner>>,
        shared: Arc<RwLock<FederatedStrategyWeights>>,
    ) -> Self {
        Self {
            peer_manager,
            self_node_id: self_node_id.into(),
            meta_learner,
            shared,
            publish_interval: Duration::from_secs(DEFAULT_PUBLISH_INTERVAL_SECS),
        }
    }

    /// 自定义发布周期（测试用）。
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.publish_interval = interval;
        self
    }

    /// 构造本地快照并写入共享视图，返回是否被接受。
    ///
    /// `generation` 取 unix 毫秒，并与「上一次本节点的 generation + 1」取大：
    /// 同一毫秒内的两次发布（测试里节拍可以很短）必须仍然严格递增，
    /// 否则第二次会被自己的合并规则拒绝，表现为「发布成功但数据不更新」。
    fn publish_once(&self) -> bool {
        let insight = {
            let learner = self.meta_learner.lock();
            learner.learn()
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let next_generation = {
            let current = self.shared.read();
            current
                .node_snapshot(&self.self_node_id)
                .map(|s| s.generation.saturating_add(1))
                .unwrap_or(0)
        };
        let generation = next_generation.max(now_ms);

        let snapshot =
            NodeStrategySnapshot::from_insight(self.self_node_id.clone(), generation, &insight);

        let payload = match serde_json::to_vec(&snapshot) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "strategy weights: failed to serialize snapshot");
                return false;
            },
        };

        let accepted = self.shared.write().merge(snapshot);
        // 广播的是「刚生成的那份」，即使本地合并被拒（理论上不会）也照样发出：
        // 对端有自己的槽位，不依赖本地的接受结果。
        self.peer_manager.broadcast_state_proposal(STRATEGY_WEIGHTS_KEY, payload);
        accepted
    }

    /// 处理一条入站 `StateProposal`，返回是否被合并。
    ///
    /// 返回 `false` 的情况都是**正常**的（key 不匹配 / 自回环 / 旧 generation），
    /// 调用方不应把它当错误。
    fn apply_remote(&self, key: &str, value: &[u8]) -> bool {
        if key != STRATEGY_WEIGHTS_KEY {
            tracing::trace!(key = %key, "strategy weights: ignoring unrelated state proposal");
            return false;
        }

        let snapshot: NodeStrategySnapshot = match serde_json::from_slice(value) {
            Ok(s) => s,
            Err(e) => {
                // 载荷来自远端：解析失败只记 warn 不 panic，也不清空本地视图。
                tracing::warn!(
                    error = %e,
                    len = value.len(),
                    "strategy weights: failed to parse remote snapshot"
                );
                return false;
            },
        };

        if snapshot.node_id == self.self_node_id {
            // 防御性自过滤：入站通道本应只有远端事件。
            tracing::trace!(node_id = %snapshot.node_id, "strategy weights: skipping own snapshot");
            return false;
        }

        let accepted = self.shared.write().merge(snapshot.clone());
        if accepted {
            tracing::debug!(
                node_id = %snapshot.node_id,
                generation = snapshot.generation,
                strategies = snapshot.strategies.len(),
                "strategy weights: merged remote snapshot"
            );
        } else {
            tracing::trace!(
                node_id = %snapshot.node_id,
                generation = snapshot.generation,
                "strategy weights: stale remote snapshot discarded"
            );
        }
        accepted
    }

    /// 启动同步循环：周期发布本地快照 + 消费入站快照。
    ///
    /// 与 [`crate::peer::ExperienceSyncHandler::spawn`] 一致，订阅的是
    /// **入站**通道：本地广播通道里的事件会被拨号器转发回集群，回环虽然
    /// 被 `generation` 幂等挡住，但会白白放大广播量。
    pub fn spawn(self, mut shutdown_rx: broadcast::Receiver<()>) -> JoinHandle<()> {
        let peer_manager = self.peer_manager.clone();
        let mut events = peer_manager.subscribe_inbound();

        let mut ticker = if self.publish_interval.is_zero() {
            None
        } else {
            let mut t = tokio::time::interval(self.publish_interval);
            // 落后时按原节拍追赶没有意义（统计不会补出历史），直接跳过。
            t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            Some(t)
        };

        tokio::spawn(async move {
            // 持有强引用：`broadcast::Receiver` 不让通道保持存活，只有 Sender
            // 存活才不会收到 `Closed`。此处与经验同步一致，避免任务静默退出。
            let _keep_manager_alive = peer_manager;
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.recv() => {
                        tracing::info!("StrategyWeightsHandler: shutdown signal received");
                        break;
                    }
                    _ = tick(&mut ticker) => {
                        self.publish_once();
                    }
                    result = events.recv() => {
                        match result {
                            Ok(MgrEvent::StateProposal { key, value }) => {
                                self.apply_remote(&key, &value);
                            },
                            Ok(_) => {
                                // 其他事件类型不处理
                            },
                            Err(broadcast::error::RecvError::Closed) => {
                                tracing::error!(
                                    "StrategyWeightsHandler: inbound channel closed, stopping"
                                );
                                break;
                            },
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                // 丢事件是正常的（入站通道容量有限）；下一拍的全量快照
                                // 会把状态重新对齐，无需补齐。
                                tracing::warn!(
                                    dropped = n,
                                    "StrategyWeightsHandler: inbound channel lagged"
                                );
                            },
                        }
                    }
                }
            }
            tracing::info!("StrategyWeightsHandler: stopped");
        })
    }
}

/// 等待下一次发布节拍；未启用发布时永久挂起。
async fn tick(ticker: &mut Option<tokio::time::Interval>) {
    match ticker {
        Some(t) => {
            t.tick().await;
        },
        None => std::future::pending::<()>().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::MetaLearningSample;
    use std::time::Duration;

    fn temp_learner(tag: &str) -> Arc<parking_lot::Mutex<MetaLearner>> {
        let dir = std::env::temp_dir().join(format!(
            "sisyphus-strategy-weights-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Arc::new(parking_lot::Mutex::new(MetaLearner::new(
            dir.to_str().expect("utf-8 temp path"),
        )))
    }

    fn sample(strategy: &str, success: bool, duration: f64) -> MetaLearningSample {
        MetaLearningSample {
            context: serde_json::json!({ "goal": "ctx" }),
            strategy: strategy.into(),
            params: serde_json::json!({}),
            success,
            duration_secs: duration,
            timestamp: 1_700_000_000,
        }
    }

    /// 空洞察（没有样本的节点）。`LearningInsight` 没有 `Default`，
    /// 这里显式构造以免测试依赖「将来会加 Default」的假设。
    fn empty_insight() -> sisyphus_types::meta_learner::LearningInsight {
        sisyphus_types::meta_learner::LearningInsight {
            total_samples: 0,
            overall_success_rate: 0.0,
            strategies: Vec::new(),
            trend: "stable".into(),
        }
    }

    fn handler(
        node: &str,
        learner: Arc<parking_lot::Mutex<MetaLearner>>,
        shared: Arc<RwLock<FederatedStrategyWeights>>,
    ) -> (Arc<PeerManager>, StrategyWeightsHandler) {
        let manager = Arc::new(PeerManager::new(node, Duration::from_secs(30)));
        let h = StrategyWeightsHandler::new(manager.clone(), node, learner, shared);
        (manager, h)
    }

    /// 发布必须同时做到两件事：本地视图更新 + 快照进入本地广播通道
    /// （拨号器/服务端只订阅这一路，没发出去等于没同步）。
    #[tokio::test]
    async fn publish_updates_shared_view_and_broadcasts() {
        let learner = temp_learner("publish");
        learner.lock().record_outcome(sample("depth-first", true, 2.0)).expect("record outcome");
        learner.lock().record_outcome(sample("depth-first", false, 4.0)).expect("record outcome");

        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (manager, h) = handler("node-a", learner, shared.clone());
        let mut local = manager.subscribe();

        assert!(h.publish_once(), "首次发布应被接受");
        assert_eq!(shared.read().node_count(), 1);

        assert_eq!(shared.read().aggregate()["depth-first"].uses, 2);
        assert_eq!(shared.read().aggregate()["depth-first"].successes, 1);
        // guard 随表达式结束即释放，不跨后续写操作
        let evt = local.recv().await.expect("本地广播通道应收到 StateProposal");
        match evt {
            MgrEvent::StateProposal { key, value } => {
                assert_eq!(key, STRATEGY_WEIGHTS_KEY);
                let snap: NodeStrategySnapshot =
                    serde_json::from_slice(&value).expect("载荷应为快照 JSON");
                assert_eq!(snap.node_id, "node-a");
                assert_eq!(snap.strategies["depth-first"].uses, 2);
            },
            other => panic!("expected StateProposal, got {other:?}"),
        }
    }

    /// 入站快照必须被合并进共享视图（这是 REST 视图能看到对端数据的前提）。
    #[test]
    fn apply_remote_merges_remote_snapshot() {
        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (_manager, h) = handler("node-a", temp_learner("merge"), shared.clone());

        let remote = NodeStrategySnapshot::from_insight(
            "node-b",
            1_700_000_000_123,
            &sisyphus_types::meta_learner::LearningInsight {
                total_samples: 3,
                overall_success_rate: 2.0 / 3.0,
                strategies: vec![sisyphus_types::meta_learner::StrategyInsight {
                    strategy: "hybrid".into(),
                    total_uses: 3,
                    successes: 2,
                    failures: 1,
                    avg_duration_secs: 1.0,
                    success_rate: 2.0 / 3.0,
                }],
                trend: "stable".into(),
            },
        );
        let payload = serde_json::to_vec(&remote).expect("serialize");

        assert!(
            h.apply_remote(STRATEGY_WEIGHTS_KEY, &payload),
            "新节点的快照必须被接受"
        );
        assert_eq!(shared.read().node_count(), 1);
        assert_eq!(shared.read().aggregate()["hybrid"].uses, 3);

        // 同一 generation 重放 -> 拒绝（幂等），计数不得翻倍。
        // 注意：此处不能持有 `shared.read()` 的 guard 跨过写操作——
        // parking_lot RwLock 不支持写期间读，持读守卫跨越写锁会自死锁。
        assert!(!h.apply_remote(STRATEGY_WEIGHTS_KEY, &payload));
        assert_eq!(shared.read().aggregate()["hybrid"].uses, 3);
    }

    /// 自回环与无关 key 都必须被忽略；损坏载荷不得 panic 也不得清空视图。
    #[test]
    fn apply_remote_rejects_self_echo_other_keys_and_garbage() {
        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (_manager, h) = handler("node-a", temp_learner("reject"), shared.clone());

        // 自己的快照（模拟被对端原样回送）
        let own = NodeStrategySnapshot::from_insight("node-a", 99, &empty_insight());
        let payload = serde_json::to_vec(&own).expect("serialize");
        assert!(
            !h.apply_remote(STRATEGY_WEIGHTS_KEY, &payload),
            "自回环必须被忽略"
        );
        assert_eq!(shared.read().node_count(), 0);

        // 其他用途的 StateProposal
        let foreign = serde_json::to_vec(&NodeStrategySnapshot::from_insight(
            "node-b",
            1,
            &empty_insight(),
        ))
        .expect("serialize");
        assert!(
            !h.apply_remote("some.other/v1", &foreign),
            "无关 key 必须被忽略"
        );

        // 损坏载荷
        assert!(
            !h.apply_remote(STRATEGY_WEIGHTS_KEY, b"not json"),
            "损坏载荷不得被接受"
        );
        assert_eq!(shared.read().node_count(), 0);
    }

    /// 现有快照不会被「同一节点更旧的 generation」覆盖（乱序到达）。
    #[test]
    fn apply_remote_keeps_newer_local_generation() {
        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (_manager, h) = handler("node-a", temp_learner("stale"), shared.clone());

        let make = |generation: u64, uses: u64| {
            let mut snap = NodeStrategySnapshot {
                node_id: "node-b".into(),
                generation,
                strategies: Default::default(),
            };
            snap.strategies.insert(
                "depth-first".into(),
                sisyphus_types::strategy_weights::StrategyStat {
                    uses,
                    successes: uses,
                    avg_duration_secs: 1.0,
                },
            );
            serde_json::to_vec(&snap).expect("serialize")
        };

        assert!(h.apply_remote(STRATEGY_WEIGHTS_KEY, &make(500, 10)));
        assert!(
            !h.apply_remote(STRATEGY_WEIGHTS_KEY, &make(100, 1)),
            "更旧的 generation 必须被丢弃"
        );
        assert_eq!(shared.read().aggregate()["depth-first"].uses, 10);
    }

    /// 周期发布后对端能收到（同一进程内两个 manager 用广播通道直连，
    /// 传输层的真实 gRPC 传播由 `tests/strategy_weights_sync.rs` 覆盖）。
    #[tokio::test]
    async fn spawn_publishes_periodically_and_stops_on_shutdown() {
        let learner = temp_learner("spawn");
        learner.lock().record_outcome(sample("hybrid", true, 1.0)).expect("record outcome");
        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (manager, h) = handler("node-a", learner, shared.clone());

        let mut local = manager.subscribe();
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let handle = h.with_interval(Duration::from_millis(20)).spawn(shutdown_rx);

        let first = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(MgrEvent::StateProposal { key, .. }) = local.recv().await {
                    break key;
                }
            }
        })
        .await
        .expect("应在超时前发布一次");
        assert_eq!(first, STRATEGY_WEIGHTS_KEY);
        assert_eq!(shared.read().node_count(), 1);

        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("收到关闭信号后应退出")
            .expect("任务不应 panic");
    }
}
