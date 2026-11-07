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
//!
//! 入站快照在**被接受后**会再转发一次（见 [`StrategyWeightsHandler::apply_remote`]），
//! 因此链式/星形拓扑也能收敛，而不要求全连接。幂等规则同时充当回环断路器。

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
    /// 上次发布的快照内容；用于跳过「统计没变化」的重复广播（v0.4.0 Phase F）。
    ///
    /// 发布周期（默认 60s）理论上是分钟级的，但两次发布内容相同仍会照发：
    /// 每次照发都会把一份无变化的快照 fan-out 给整个集群（全连接时 O(N²)）。
    /// 内容（策略统计）未变时跳过发送，只更新本地视图的 generation。
    last_published: parking_lot::Mutex<Option<NodeStrategySnapshot>>,
    /// 已知成员集合（治本对账用）。启动时从 manager 同步一次现场成员，
    /// Join/Leave 成员事件在此更新。对账只回收「曾见过成员、现在已离线」
    /// 的槽位，避免清洗从未见过的节点。
    seen_members: parking_lot::Mutex<std::collections::HashSet<String>>,
}

impl StrategyWeightsHandler {
    /// 创建处理器（默认 60 秒发布一次）。
    pub fn new(
        peer_manager: Arc<PeerManager>,
        self_node_id: impl Into<String>,
        meta_learner: Arc<parking_lot::Mutex<MetaLearner>>,
        shared: Arc<RwLock<FederatedStrategyWeights>>,
    ) -> Self {
        let seen_members = std::collections::HashSet::from_iter(
            peer_manager.list_peers().into_iter().map(|p| p.node_id),
        );
        Self {
            peer_manager,
            self_node_id: self_node_id.into(),
            meta_learner,
            shared,
            publish_interval: Duration::from_secs(DEFAULT_PUBLISH_INTERVAL_SECS),
            last_published: parking_lot::Mutex::new(None),
            seen_members: parking_lot::Mutex::new(seen_members),
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

        let accepted = self.shared.write().merge(snapshot.clone());
        // 广播的是「刚生成的那份」，即使本地合并被拒（理论上不会）也照样发出：
        // 对端有自己的槽位，不依赖本地的接受结果。
        //
        // Phase F：本地快照内容未变化时跳过广播（仅更新本节点视图 generation），
        // 避免每拍照发一份无变化的快照把全连接集群的 O(N²) 放大滚雪球。
        let changed = {
            let mut last = self.last_published.lock();
            let changed =
                last.as_ref().map(|s| s.strategies != snapshot.strategies).unwrap_or(true);
            if changed {
                *last = Some(snapshot.clone());
            }
            changed
        };
        if changed {
            self.peer_manager.broadcast_state_proposal(
                self.self_node_id.clone(),
                STRATEGY_WEIGHTS_KEY,
                payload,
            );
        }
        accepted
    }

    /// 节点离开时回收其槽位，返回是否真的移除了槽位（ADR-017 Amendment 2）。
    ///
    /// 语义见 [`FederatedStrategyWeights::remove_node`]：视图代表**当前**集群，
    /// 不保留已下线节点的历史累计。分区期间会先移除、重连后由下一拍快照加回。
    fn forget_node(&self, node_id: &str) -> bool {
        let removed = self.shared.write().remove_node(node_id);
        if removed {
            tracing::info!(node_id = %node_id, "strategy weights: dropped slot of departed node");
        } else {
            tracing::trace!(
                node_id = %node_id,
                "strategy weights: peer left without a strategy slot"
            );
        }
        removed
    }

    /// 处理一条入站 `StateProposal`，返回是否被合并。
    ///
    /// 返回 `false` 的情况都是**正常**的（key 不匹配 / 自回环 / 旧 generation /
    /// 来源与载荷不一致），调用方不应把它当错误。
    ///
    /// `source_node_id` 是**线上声明**的来源（v0.4.0 Phase G）。它有两处用途：
    /// 1. 在解析 JSON **之前**识别自回环——省掉一次全量解析，也让「谁在回送
    ///    我的快照」在 trace 日志里可定位；
    /// 2. 与载荷里的 `snapshot.node_id` 交叉校验。载荷是唯一的事实来源（视图
    ///    按它分槽），但「事件声称来自 A、载荷却写着 B」只可能是对端实现有 bug
    ///    或在伪造来源，两种都不该并入视图。
    ///
    /// 空 `source_node_id` 表示对端是旧版本（字段默认值），此时跳过交叉校验，
    /// 只按载荷处理——滚动升级期间不能因为字段缺失就丢掉对端数据。
    fn apply_remote(&self, source_node_id: &str, key: &str, value: &[u8]) -> bool {
        if key != STRATEGY_WEIGHTS_KEY {
            tracing::trace!(key = %key, "strategy weights: ignoring unrelated state proposal");
            return false;
        }

        if !source_node_id.is_empty() && source_node_id == self.self_node_id {
            // 线上来源即自己：这条事件必然是被对端原样回送的，不必解析。
            tracing::trace!(
                node_id = %source_node_id,
                "strategy weights: skipping own snapshot by wire source"
            );
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

        if !source_node_id.is_empty() && snapshot.node_id != source_node_id {
            // 来源与载荷不一致：视图按 `snapshot.node_id` 分槽，若放过就会让
            // 事件声称的来源与实际归属永久错位（定向转发的判断依据也会失真）。
            tracing::warn!(
                wire_source = %source_node_id,
                payload_node = %snapshot.node_id,
                generation = snapshot.generation,
                "strategy weights: dropping snapshot whose payload disagrees with wire source"
            );
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
            // 多跳传播（ADR-017 Amendment）：**只在「新一代被接受」时**转发。
            //
            // 不转发就无法覆盖静态 peer 的常见拓扑：`peers` 通常只列种子节点
            // （星形），叶子之间没有直连流，叶子 A 的快照永远到不了叶子 B。
            // 直连可达性因此不是「收敛」的前提。
            //
            // 转发次数有界、不成环，靠三条已有的性质：
            // 1. 同一节点对同一 generation 只接受一次 → 同一份快照在每节点
            //    至多转发一次（重复到达不会再触发转发）；
            // 2. 快照回到源节点时被 `self_node_id` 自过滤挡下；
            // 3. generation 严格递增 → 不存在「越转越新」的正反馈。
            //
            // 代价是多跳拓扑下每条边可能多传一次同一份快照（O(节点数) 跳、
            // 全连接时 O(N²) 次发送）。载荷是分钟级的策略统计，可接受。
            self.peer_manager.broadcast_state_proposal(
                snapshot.node_id.clone(),
                STRATEGY_WEIGHTS_KEY,
                value.to_vec(),
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

    /// ADR-017 遗留的治本方案：用 `list_peers()` 对账视图里的节点集合，
    /// 回收「曾见过成员、现在已离线的槽位」。
    ///
    /// 背景：成员事件只走本地通道；订阅者因广播 `Lagged` 丢掉了 `PeerLeft`
    /// 时，`forget_node` 没被触发，视图里就会残留已离线的槽位（聚合被污染）。
    /// 与周期发布并列跑一次本对账，不依赖事件恰好送达。
    ///
    /// 只回收 `seen_members` 里记录过、但已不在当前成员集合里的节点槽位
    /// （自身除外）；从未见过（`seen_members` 未收录）的槽位一律保留，避免清洗
    /// 异常状态下的正常远端数据。
    ///
    /// 幂等：对账只删槽位，不重建。
    fn reconcile_offline_nodes(&self) -> usize {
        let live: std::collections::HashSet<String> = self
            .peer_manager
            .list_peers()
            .into_iter()
            .map(|p| p.node_id)
            .chain(std::iter::once(self.self_node_id.clone()))
            .collect();
        let seen = self.seen_members.lock().clone();
        let mut dropped = 0usize;
        let managed: Vec<String> = self.shared.read().node_ids();
        for node_id in managed {
            if node_id == self.self_node_id {
                continue;
            }
            let known = seen.contains(&node_id);
            let online = live.contains(&node_id);
            if known && !online && self.shared.write().remove_node(&node_id) {
                dropped += 1;
                tracing::info!(
                    node_id = %node_id,
                    "strategy weights: reconciled stale slot after member lag"
                );
            }
        }
        *self.seen_members.lock() = live;
        dropped
    }

    /// 启动同步循环：周期发布本地快照 + 消费入站快照。
    ///
    /// 与 [`crate::peer::ExperienceSyncHandler::spawn`] 一致，订阅的是
    /// **入站**通道：本地广播通道里的事件会被拨号器转发回集群，回环虽然
    /// 被 `generation` 幂等挡住，但会白白放大广播量。
    pub fn spawn(self, mut shutdown_rx: broadcast::Receiver<()>) -> JoinHandle<()> {
        let peer_manager = self.peer_manager.clone();
        let mut events = peer_manager.subscribe_inbound();
        // 成员事件（PeerLeft/PeerJoined）只出现在**本地**通道上：
        // `on_peer_left` 与 `evict_expired` 都发到 `event_tx`，
        // 入站通道 `inbound_tx` 里根本没有它们。只订阅入站会让「节点下线后
        // 回收槽位」变成永远不触发——又一个「代码看着接好了」的坑。
        let mut members = peer_manager.subscribe();

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
                // ADR-017 遗留治本方案：周期性对账成员集合，回收漏掉 `PeerLeft` 的槽位。
                self.reconcile_offline_nodes();
                tokio::select! {
                    biased;
                    _ = shutdown_rx.recv() => {
                        tracing::info!("StrategyWeightsHandler: shutdown signal received");
                        break;
                    }
                    _ = tick(&mut ticker) => {
                        self.publish_once();
                    }
                    result = members.recv() => {
                        match result {
                            Ok(MgrEvent::PeerJoined(info)) => {
                                self.seen_members.lock().insert(info.node_id.clone());
                            },
                            Ok(MgrEvent::PeerLeft { node_id, reason }) => {
                                tracing::debug!(
                                    node_id = %node_id,
                                    reason = %reason,
                                    "strategy weights: peer left"
                                );
                                self.seen_members.lock().remove(&node_id);
                                self.forget_node(&node_id);
                            },
                            // 本地通道上还会看到自己发布的 StateProposal（publish_once
                            // 走 event_tx）。策略合并只认入站事件，这里必须忽略，
                            // 否则每拍都会把自己的快照再处理一遍。
                            Ok(_) => {},
                            Err(broadcast::error::RecvError::Closed) => {
                                tracing::error!(
                                    "StrategyWeightsHandler: local channel closed, stopping"
                                );
                                break;
                            },
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                // 丢成员事件意味着可能漏掉 PeerLeft → 槽位残留。
                                // 记 warn 而不是静默：残留会体现为聚合里多一个节点。
                                tracing::warn!(
                                    dropped = n,
                                    "StrategyWeightsHandler: local channel lagged"
                                );
                            },
                        }
                    }
                    result = events.recv() => {
                        match result {
                            Ok(MgrEvent::StateProposal { source_node_id, key, value }) => {
                                self.apply_remote(&source_node_id, &key, &value);
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
            MgrEvent::StateProposal { key, value, .. } => {
                assert_eq!(key, STRATEGY_WEIGHTS_KEY);
                let snap: NodeStrategySnapshot =
                    serde_json::from_slice(&value).expect("载荷应为快照 JSON");
                assert_eq!(snap.node_id, "node-a");
                assert_eq!(snap.strategies["depth-first"].uses, 2);
            },
            other => panic!("expected StateProposal, got {other:?}"),
        }
    }

    /// Phase F：本地快照内容未变化时跳过广播（不发无变化的快照）。
    ///
    /// 与「必须广播」的单测互补：前者锁住「首次发布照发」，后者锁住「同内容
    /// 二次发布不照发」。计数走 `PeerManager::state_proposal_broadcasts`，
    /// 与实际 outbound 量同源，避免在 handler 里再造一座计数器。
    #[tokio::test]
    async fn publish_skips_broadcast_when_snapshot_unchanged() {
        let learner = temp_learner("noop");
        learner.lock().record_outcome(sample("depth-first", true, 2.0)).expect("record outcome");

        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (manager, h) = handler("node-a", learner, shared.clone());

        assert!(h.publish_once(), "首次发布应被接受");
        assert_eq!(
            manager.state_proposal_broadcasts(),
            1,
            "首次发布内容变化，照发"
        );

        // 同一次学习周期内第二次 publish：generation 会变（自增），但
        // 策略统计内容不变 → 必须跳过广播，不得把无变化的快照又送一遍集群。
        assert!(
            h.publish_once(),
            "内容未变时仍应返回接受（generation 更新）"
        );
        assert_eq!(
            manager.state_proposal_broadcasts(),
            1,
            "统计内容未变化时不得再广播"
        );
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
            h.apply_remote("node-b", STRATEGY_WEIGHTS_KEY, &payload),
            "新节点的快照必须被接受"
        );
        assert_eq!(shared.read().node_count(), 1);
        assert_eq!(shared.read().aggregate()["hybrid"].uses, 3);

        // 同一 generation 重放 -> 拒绝（幂等），计数不得翻倍。
        // 注意：此处不能持有 `shared.read()` 的 guard 跨过写操作——
        // parking_lot RwLock 不支持写期间读，持读守卫跨越写锁会自死锁。
        assert!(!h.apply_remote("node-b", STRATEGY_WEIGHTS_KEY, &payload));
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
        // 线上来源即自己：无论载荷是否合法都必须被忽略（实现上在解析前早退，
        // 但早退本身不改变可观测结果，这里锁的是行为而不是路径）。
        assert!(
            !h.apply_remote("node-a", STRATEGY_WEIGHTS_KEY, b"not even json"),
            "线上来源为本节点时必须被忽略"
        );
        // 空来源（旧版本对端）+ 载荷是自己：仍要被载荷级自过滤挡下。
        assert!(
            !h.apply_remote("", STRATEGY_WEIGHTS_KEY, &payload),
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
            !h.apply_remote("node-b", "some.other/v1", &foreign),
            "无关 key 必须被忽略"
        );

        // 损坏载荷
        assert!(
            !h.apply_remote("node-b", STRATEGY_WEIGHTS_KEY, b"not json"),
            "损坏载荷不得被接受"
        );
        assert_eq!(shared.read().node_count(), 0);
    }

    /// Phase G：线上声明的来源必须与载荷里的 `node_id` 一致。
    ///
    /// 视图按载荷的 `node_id` 分槽，而「来源」是转发与去重判断的依据。两者
    /// 不一致只可能是对端实现有 bug 或在伪造来源——两种都不该并入视图，
    /// 否则同一节点会被记到别人的槽位上，且永远无法自愈（后续 generation
    /// 更高的快照只会让错误归属更牢固）。
    #[test]
    fn apply_remote_rejects_wire_source_payload_mismatch() {
        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (_manager, h) = handler("node-a", temp_learner("mismatch"), shared.clone());

        let payload = serde_json::to_vec(&NodeStrategySnapshot::from_insight(
            "node-b",
            100,
            &empty_insight(),
        ))
        .expect("serialize");

        assert!(
            !h.apply_remote("node-c", STRATEGY_WEIGHTS_KEY, &payload),
            "来源与载荷不一致必须被拒绝"
        );
        assert_eq!(shared.read().node_count(), 0, "视图里不得留下任何槽位");

        // 同一载荷、来源一致时必须被接受——证明拒绝的是不一致，而不是载荷本身。
        assert!(h.apply_remote("node-b", STRATEGY_WEIGHTS_KEY, &payload));
        assert_eq!(shared.read().node_count(), 1);
    }

    /// 滚动升级：旧版本对端的 `source_node_id` 是空字符串（proto3 默认值）。
    ///
    /// 此时**不能**当成「来源不一致」丢掉——那会让升级期间的对端数据全部消失。
    /// 空来源只跳过交叉校验，载荷仍按原有语义处理。
    #[test]
    fn apply_remote_accepts_empty_wire_source_for_backward_compat() {
        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (_manager, h) = handler("node-a", temp_learner("compat"), shared.clone());

        let payload = serde_json::to_vec(&NodeStrategySnapshot::from_insight(
            "node-b",
            100,
            &empty_insight(),
        ))
        .expect("serialize");

        assert!(
            h.apply_remote("", STRATEGY_WEIGHTS_KEY, &payload),
            "空来源（旧版本对端）必须被接受"
        );
        assert_eq!(shared.read().node_count(), 1);
    }

    /// ADR-017 遗留治本方案：对账只回收「曾见过成员、现在已离线的槽位」。
    ///
    /// 已知成员 `node-b` 已离（不在当前成员集合），槽位必须被回收；
    /// 从未见过的节点槽位不参与清理（避免误删）。
    #[test]
    fn reconcile_offline_nodes_drops_orphaned_known_slots() {
        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (_manager, h) = handler("node-a", temp_learner("reconcile"), shared.clone());

        let remote = NodeStrategySnapshot::from_insight("node-b", 10, &empty_insight());
        assert!(shared.write().merge(remote));
        assert_eq!(shared.read().node_count(), 1);

        // 模拟「曾见过 node-b 但已离线」：成员簿里不含 node-b，而 seen 里有。
        h.seen_members.lock().insert("node-b".to_string());

        let dropped = h.reconcile_offline_nodes();
        assert_eq!(dropped, 1, "已离线的 known 成员槽位应被回收");
        assert_eq!(shared.read().node_count(), 0);

        // 从未见过的节点：不清理。
        let remote2 = NodeStrategySnapshot::from_insight("node-zzz", 3, &empty_insight());
        assert!(shared.write().merge(remote2));
        assert_eq!(shared.read().node_count(), 1);
        assert_eq!(h.reconcile_offline_nodes(), 0, "从未见过的节点不应被回收");
        assert_eq!(shared.read().node_count(), 1);
    }

    /// 节点离开后必须回收其槽位，且只影响该节点（ADR-017 Amendment 2）。
    ///
    /// 走真实装配：`on_peer_left` 把 `PeerLeft` 发到本地通道，handler 的
    /// `members` 分支消费它。只测 `remove_node` 是不够的——入站通道里
    /// 根本没有成员事件，若忘了订阅本地通道，单测全绿和功能永不触发.
    #[tokio::test]
    async fn peer_left_drops_node_slot_and_keeps_others() {
        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (manager, h) = handler("node-a", temp_learner("ttl"), shared.clone());

        // 视图里已有两个远端节点（模拟此前收到的快照）。
        let remote = |node: &str| NodeStrategySnapshot::from_insight(node, 10, &empty_insight());
        assert!(shared.write().merge(remote("node-b")));
        assert!(shared.write().merge(remote("node-c")));
        assert_eq!(shared.read().node_count(), 2);

        // 先让 node-b 进入成员表，否则 `on_peer_left` 不会发事件。
        assert!(
            manager.on_peer_joined(crate::peer::manager::PeerInfo::new("node-b", "127.0.0.1:1"))
        );

        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let _handle = h.with_interval(Duration::ZERO).spawn(shutdown_rx);

        manager.on_peer_left("node-b", "test_disconnect");

        let dropped = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if shared.read().node_count() == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(
            dropped.is_ok(),
            "PeerLeft 必须触发槽位回收，实际仍剩 {} 个节点",
            shared.read().node_count()
        );
        assert!(
            shared.read().node_snapshot("node-c").is_some(),
            "只应移除 node-b，node-c 的槽位必须保留"
        );
        drop(shutdown_tx);
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

        assert!(h.apply_remote("node-b", STRATEGY_WEIGHTS_KEY, &make(500, 10)));
        assert!(
            !h.apply_remote("node-b", STRATEGY_WEIGHTS_KEY, &make(100, 1)),
            "更旧的 generation 必须被丢弃"
        );
        assert_eq!(shared.read().aggregate()["depth-first"].uses, 10);
    }

    /// 被接受的入站快照必须再转发一次，被拒的不得转发（ADR-017 Amendment）。
    ///
    /// 转发是链式/星形拓扑能收敛的唯一原因：叶子之间没有直连流，快照必须
    /// 经中间节点搬运。同时它必须**恰好**跟着「接受」发生——否则拒绝路径
    /// （重放 / 自回环 / 旧 generation）会变成广播风暴的源头。
    #[tokio::test]
    async fn apply_remote_forwards_only_accepted_snapshots() {
        let shared = Arc::new(RwLock::new(FederatedStrategyWeights::default()));
        let (manager, h) = handler("node-a", temp_learner("forward"), shared.clone());
        let mut local = manager.subscribe();

        let remote = NodeStrategySnapshot::from_insight("node-b", 500, &empty_insight());
        let payload = serde_json::to_vec(&remote).expect("serialize");

        assert!(
            h.apply_remote("node-b", STRATEGY_WEIGHTS_KEY, &payload),
            "首次到达应被接受"
        );
        let forwarded = tokio::time::timeout(Duration::from_secs(2), local.recv())
            .await
            .expect("转发事件应出现在本地广播通道")
            .expect("本地广播通道应可读");
        match forwarded {
            MgrEvent::StateProposal { key, value, .. } => {
                assert_eq!(key, STRATEGY_WEIGHTS_KEY);
                assert_eq!(
                    value, payload,
                    "转发必须原样搬运载荷，而不是重新序列化本地视图"
                );
            },
            other => panic!("expected forwarded StateProposal, got {other:?}"),
        }

        // 同 generation 重放：拒绝，且**不得**再转发。
        assert!(
            !h.apply_remote("node-b", STRATEGY_WEIGHTS_KEY, &payload),
            "同 generation 重放应被拒绝"
        );
        match local.try_recv() {
            Err(broadcast::error::TryRecvError::Empty) => {},
            Ok(evt) => panic!("被拒绝的快照不得转发，却收到 {evt:?}"),
            Err(e) => panic!("本地通道意外状态: {e:?}"),
        }

        // 自回环：拒绝，且不得转发（否则自己的快照会被集群原样送回）。
        let own = serde_json::to_vec(&NodeStrategySnapshot::from_insight(
            "node-a",
            900,
            &empty_insight(),
        ))
        .expect("serialize");
        assert!(
            !h.apply_remote("node-a", STRATEGY_WEIGHTS_KEY, &own),
            "自回环应被拒绝"
        );
        match local.try_recv() {
            Err(broadcast::error::TryRecvError::Empty) => {},
            Ok(evt) => panic!("自回环不得转发，却收到 {evt:?}"),
            Err(e) => panic!("本地通道意外状态: {e:?}"),
        }
    }

    /// 周期发布后对端能收到（同一进程内两个 manager 用广播通道直连，
    /// 传输层的真实 gRPC 传播由 `tests/cluster_integration.rs` 的
    /// `test_two_node_strategy_weights_propagate_and_merge` 覆盖）。
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
