use sisyphus_core::Consensus;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use async_trait::async_trait;
use std::collections::HashSet;

/// 简单的集群共识实现
pub struct ClusterConsensus {
    node_id: String,
    peers: Arc<RwLock<HashSet<String>>>,
    leader_id: Arc<RwLock<String>>,
}

impl ClusterConsensus {
    pub fn new(node_id: String, initial_peers: Vec<String>) -> Self {
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
        }
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

    async fn propose(&self, key: &str, _value: Value) -> Result<(), String> {
        if !self.is_leader().await {
            return Err("Only the leader can propose state changes".into());
        }

        let peers = self.peers.read().await;
        tracing::info!(key = %key, peer_count = peers.len(), "Broadcasting state proposal to cluster");

        // 在实际实现中，这里将通过 gRPC 调用 peers 节点的 /commit 接口
        for peer in peers.iter() {
            tracing::debug!(peer = %peer, key = %key, "Sending proposal to peer");
            // mock_grpc_call(peer, key, value).await?;
        }

        Ok(())
    }
}

/// No-op consensus implementation: always assumes this node is the leader.
#[allow(dead_code)]
pub(crate) struct NoOpConsensus;

#[async_trait]
impl Consensus for NoOpConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "self".into()
    }
    async fn propose(&self, _key: &str, _value: Value) -> Result<(), String> {
        Ok(())
    }
}
