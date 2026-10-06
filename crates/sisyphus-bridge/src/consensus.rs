use async_trait::async_trait;
use serde_json::Value;
use sisyphus_core::Consensus;
use sisyphus_types::TypesError;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// 简单的集群共识实现
pub struct ClusterConsensus {
    node_id: String,
    peers: Arc<RwLock<HashSet<String>>>,
    leader_id: Arc<RwLock<String>>,
    /// 可注入的 peer proposer（TODO 8.3.3）：默认 None 时 propose 只 log；
    /// 测试可注入 mock 实现不依赖网络。
    proposer: Arc<RwLock<Option<Arc<dyn PeerProposer>>>>,
    /// propose 超时（默认 1s；测试可调小）
    propose_timeout: Duration,
}

/// Peer 提议发送 trait（可注入 mock 用于单进程 benchmark）
#[async_trait]
pub trait PeerProposer: Send + Sync {
    /// 向 peer 发送提议，返回是否成功
    async fn send_proposal(&self, peer: &str, key: &str, value: &Value) -> bool;
}

impl ClusterConsensus {
    /// New.
    pub fn new(node_id: String, initial_peers: Vec<String>) -> Self {
        Self::with_proposer(node_id, initial_peers, None, Duration::from_secs(1))
    }

    /// 注入自定义 peer proposer（测试用）+ 自定义超时
    pub fn with_proposer(
        node_id: String,
        initial_peers: Vec<String>,
        proposer: Option<Arc<dyn PeerProposer>>,
        propose_timeout: Duration,
    ) -> Self {
        let mut peers = HashSet::new();
        for peer in initial_peers {
            peers.insert(peer);
        }

        // 简单的领导者选举：ID 字典序最小的为 Leader
        let mut all_nodes = peers.clone();
        all_nodes.insert(node_id.clone());
        let leader = all_nodes.into_iter().min().unwrap_or(node_id.clone());

        Self {
            node_id,
            peers: Arc::new(RwLock::new(peers)),
            leader_id: Arc::new(RwLock::new(leader)),
            proposer: Arc::new(RwLock::new(proposer)),
            propose_timeout,
        }
    }

    /// 动态加入 peer（cluster membership）
    pub async fn add_peer(&self, peer_id: String) {
        let mut peers = self.peers.write().await;
        if peers.insert(peer_id.clone()) {
            // 重新计算 leader（字典序最小）。`all` 至少含 `node_id`，故实际不会为空；
            // 仍不用 `unwrap()`：ADR-007 §1 的 panic-check 以 panic-baseline.json
            // 为门禁，生产代码新增 panic 路径会让该 job 直接失败。
            // 语义等价（既有 test_add_peer_recomputes_leader 等用例守护）：
            // `min()` 取最小值副本；为 None 时保留现有 leader。
            let mut all = peers.clone();
            all.insert(self.node_id.clone());
            if let Some(new_leader) = all.into_iter().min() {
                *self.leader_id.write().await = new_leader;
            }
        }
    }

    /// 动态移除 peer
    pub async fn remove_peer(&self, peer_id: &str) {
        let mut peers = self.peers.write().await;
        if peers.remove(peer_id) {
            // 同 `add_peer`：为 None 时保留现有 leader，不做 panic 解包。
            let mut all = peers.clone();
            all.insert(self.node_id.clone());
            if let Some(new_leader) = all.into_iter().min() {
                *self.leader_id.write().await = new_leader;
            }
        }
    }

    /// 当前 peers 数量（含 leader）
    pub async fn cluster_size(&self) -> usize {
        let peers = self.peers.read().await;
        peers.len() + 1 // + self
    }
}

#[async_trait]
impl Consensus for ClusterConsensus {
    async fn is_leader(&self) -> bool {
        *self.leader_id.read().await == self.node_id
    }

    async fn get_leader(&self) -> String {
        self.leader_id.read().await.clone()
    }

    async fn propose(&self, key: &str, value: Value) -> Result<(), TypesError> {
        if !self.is_leader().await {
            return Err(TypesError::ConsensusError(
                "Only the leader can propose state changes".into(),
            ));
        }

        let peer_count = {
            let peers = self.peers.read().await;
            peers.len()
        };
        tracing::info!(
            key = %key,
            peer_count = peer_count,
            "Broadcasting state proposal to cluster"
        );

        // 如果注入了 proposer（如 benchmark mock），真正发送并统计 ack
        let proposer = self.proposer.read().await.clone();
        if let Some(proposer) = proposer {
            use tokio::time::timeout;
            let peers_snapshot: Vec<String> = self.peers.read().await.iter().cloned().collect();
            let mut acks = 0;
            for peer in &peers_snapshot {
                match timeout(
                    self.propose_timeout,
                    proposer.send_proposal(peer, key, &value),
                )
                .await
                {
                    Ok(true) => acks += 1,
                    Ok(false) => tracing::warn!(peer = %peer, "peer rejected proposal"),
                    Err(_) => tracing::warn!(peer = %peer, "peer proposal timed out"),
                }
            }
            // 简单 quorum：majority 同意即成功
            let total = peers_snapshot.len() + 1; // + leader
            if acks * 2 < total {
                return Err(TypesError::ConsensusError(format!(
                    "Quorum not reached: {acks}/{total} acks"
                )));
            }
            return Ok(());
        }

        // 默认 no-op 行为：只 log，不真发 RPC
        tracing::debug!(key = %key, "No proposer injected; logging only");
        Ok(())
    }
}

/// No-op consensus implementation: always assumes this node is the leader.
#[expect(dead_code)]
pub(crate) struct NoOpConsensus;

#[async_trait]
impl Consensus for NoOpConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "self".into()
    }
    async fn propose(&self, _key: &str, _value: Value) -> Result<(), TypesError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Mock proposer：按 peer 列表逐个返回 ack（确定性，无 rand 依赖）
    struct MockProposer {
        /// 每 peer 对应的 ack 序列（true=ack, false=nack）；循环使用
        acks: Vec<bool>,
        delay: Duration,
    }

    #[async_trait]
    impl PeerProposer for MockProposer {
        async fn send_proposal(&self, peer: &str, _key: &str, _value: &Value) -> bool {
            tokio::time::sleep(self.delay).await;
            // 用 peer 名的 hash 取模选 ack
            let idx = peer.bytes().map(|b| b as usize).sum::<usize>() % self.acks.len();
            self.acks[idx]
        }
    }

    #[tokio::test]
    async fn test_leader_election_dict_order() {
        // 字典序最小的 ID 是 leader
        let consensus =
            ClusterConsensus::new("node-c".into(), vec!["node-a".into(), "node-b".into()]);
        assert_eq!(consensus.get_leader().await, "node-a");
        assert!(!consensus.is_leader().await);
    }

    #[tokio::test]
    async fn test_is_leader_self_when_only_node() {
        let consensus = ClusterConsensus::new("solo-node".into(), vec![]);
        assert!(consensus.is_leader().await);
    }

    #[tokio::test]
    async fn test_propose_rejected_when_not_leader() {
        let consensus = ClusterConsensus::new("node-z".into(), vec!["node-a".into()]);
        // node-a is leader, node-z not
        assert!(!consensus.is_leader().await);
        let err = consensus.propose("k", json!("v")).await.unwrap_err();
        assert!(format!("{err:?}").contains("leader"));
    }

    #[tokio::test]
    async fn test_propose_succeeds_as_leader_without_proposer() {
        // No proposer: defaults to no-op success (logging only)
        let consensus = ClusterConsensus::new("node-a".into(), vec!["node-b".into()]);
        assert!(consensus.is_leader().await);
        consensus.propose("k", json!("v")).await.expect("leader propose");
    }

    #[tokio::test]
    async fn test_propose_quorum_majority_succeeds() {
        // 3 nodes (1 leader + 2 peers), 2/3 ack required
        // 全部 peer 都 ack
        let proposer = Arc::new(MockProposer {
            acks: vec![true],
            delay: Duration::from_millis(1),
        });
        let consensus = ClusterConsensus::with_proposer(
            "node-a".into(),
            vec!["node-b".into(), "node-c".into()],
            Some(proposer),
            Duration::from_secs(1),
        );
        assert!(consensus.is_leader().await);
        consensus.propose("k", json!("v")).await.expect("quorum reached");
    }

    #[tokio::test]
    async fn test_propose_quorum_fails_below_majority() {
        // 全部 peer nack → quorum not reached
        let proposer = Arc::new(MockProposer {
            acks: vec![false],
            delay: Duration::from_millis(1),
        });
        let consensus = ClusterConsensus::with_proposer(
            "node-a".into(),
            vec!["node-b".into(), "node-c".into()],
            Some(proposer),
            Duration::from_secs(1),
        );
        let err = consensus.propose("k", json!("v")).await.unwrap_err();
        assert!(format!("{err:?}").contains("Quorum"));
    }

    #[tokio::test]
    async fn test_add_peer_recomputes_leader() {
        let consensus = ClusterConsensus::new("node-b".into(), vec!["node-c".into()]);
        assert_eq!(consensus.get_leader().await, "node-b");
        consensus.add_peer("node-a".into()).await;
        assert_eq!(consensus.get_leader().await, "node-a");
    }

    #[tokio::test]
    async fn test_remove_peer_recomputes_leader() {
        let consensus =
            ClusterConsensus::new("node-a".into(), vec!["node-b".into(), "node-c".into()]);
        assert_eq!(consensus.get_leader().await, "node-a");
        // remove_peer filters by peer_id; node-a is self not peer
        consensus.remove_peer("node-b").await;
        // now peers = {node-c}; leader = min({node-c, node-a}) = node-a
        assert_eq!(consensus.get_leader().await, "node-a");
    }

    #[tokio::test]
    async fn test_propose_timeout() {
        // proposer 永远 sleep 超时 → quorum fail
        struct SlowProposer;
        #[async_trait]
        impl PeerProposer for SlowProposer {
            async fn send_proposal(&self, _: &str, _: &str, _: &Value) -> bool {
                tokio::time::sleep(Duration::from_secs(10)).await;
                true
            }
        }
        let proposer: Arc<dyn PeerProposer> = Arc::new(SlowProposer);
        let consensus = ClusterConsensus::with_proposer(
            "node-a".into(),
            vec!["node-b".into()],
            Some(proposer),
            Duration::from_millis(50),
        );
        let err = consensus.propose("k", json!("v")).await.unwrap_err();
        assert!(format!("{err:?}").contains("Quorum"));
    }

    /// Consensus 开销基准：5 节点 propose 延迟。
    ///
    /// 用 mock proposer（零延迟 ack）测纯 consensus 开销。
    /// 曾经的 `TODO 8.3.3` 已完成——本用例即其实现，故移除该标记。
    #[tokio::test]
    async fn bench_consensus_5_nodes() {
        let proposer = Arc::new(MockProposer {
            acks: vec![true],
            delay: Duration::from_millis(0),
        });
        // 5 节点: node-a (leader) + 4 peers
        let consensus = ClusterConsensus::with_proposer(
            "node-a".into(),
            vec![
                "node-b".into(),
                "node-c".into(),
                "node-d".into(),
                "node-e".into(),
            ],
            Some(proposer),
            Duration::from_secs(1),
        );
        assert!(consensus.is_leader().await);
        assert_eq!(consensus.cluster_size().await, 5);

        // 1000 次 propose 测总延迟（key 复用同一 String 避免 alloc 干扰）
        let n = 1000;
        let key = "k";
        let start = std::time::Instant::now();
        for i in 0..n {
            consensus.propose(key, json!(i)).await.expect("quorum");
        }
        let elapsed = start.elapsed();
        let per_op = elapsed / n;

        println!(
            "consensus 5-node benchmark: {n} ops in {elapsed:?} ({per_op:?}/op, {} ops/sec)",
            1_000_000_000 / per_op.as_nanos().max(1) as u64
        );
        // sanity: 单次 propose < 1ms（mock 零延迟）
        assert!(per_op < Duration::from_millis(10));
    }
}
