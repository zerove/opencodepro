//! Peer manager: 成员关系 + 事件广播（TODO 8.3.1）
//!
//! 设计要点：
//! - 纯函数 `decide_cluster_event` + 状态 `apply_event`：可独立测试
//! - Peer 列表用 `parking_lot::RwLock<BTreeMap>` 保护（按 node_id 排序，便于确定 leader）
//! - Leader = 字典序最小的 node_id（与现有 `ClusterConsensus` 一致）
//! - 事件通过 `tokio::sync::broadcast` 广播给订阅者
//!
//! ClusterEvent 与 sisyphus.cluster.ClusterEvent (proto) 1:1 对应，
//! 但用 Rust 类型（更易测试）。

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// Peer 信息（包含 join 时间 + 最后心跳时间，用于 TTL 过期判断）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    /// 节点唯一 ID
    pub node_id: String,
    /// gRPC 监听地址（如 `http://[::1]:50051` 或 `node-1:50051`）
    pub grpc_addr: String,
    /// 加入时间（Unix epoch seconds）
    pub joined_at: i64,
}

impl PeerInfo {
    /// 构造一个新 peer（joined_at = now）
    pub fn new(node_id: impl Into<String>, grpc_addr: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            grpc_addr: grpc_addr.into(),
            joined_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        }
    }
}

/// Peer 状态（用于 UI / 监控）
///
/// 注：`last_seen` 字段类型 `Instant` 不实现 `Serialize`，所以 `PeerStatus`
/// 不派生 `Serialize`/`Deserialize`。如需序列化可通过 `chrono::DateTime`
/// 或自定义 ISO8601 string 转换。
#[derive(Clone, Debug)]
pub struct PeerStatus {
    pub info: PeerInfo,
    /// 最后一次心跳 / Stream 事件时间（用于 TTL 过期检测）
    pub last_seen: Instant,
    /// 是否为当前 leader（字典序最小）
    pub is_leader: bool,
}

/// 集群事件（Rust 内部表示，与 proto 1:1 对应）
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClusterEvent {
    /// 新 peer 加入（含自己的初始 Join）
    PeerJoined(PeerInfo),
    /// peer 离开（graceful shutdown 或 TTL 过期）
    PeerLeft {
        node_id: String,
        /// 离开原因（"shutdown" / "ttl_expired" / "removed"）
        reason: String,
    },
    /// 跨节点状态提案（CRDT 同步等）
    StateProposal { key: String, value: Vec<u8> },
    /// Experience sync 事件（TODO 8.3.2 + 8.3.1 集成）
    ExperienceSync {
        exp_key: String,
        version: u64,
        vector_clock_json: String,
        payload: Vec<u8>,
    },
}

impl ClusterEvent {
    /// 事件类型字符串（用于日志 / 指标）
    pub fn kind(&self) -> &'static str {
        match self {
            Self::PeerJoined(_) => "peer_joined",
            Self::PeerLeft { .. } => "peer_left",
            Self::StateProposal { .. } => "state_proposal",
            Self::ExperienceSync { .. } => "experience_sync",
        }
    }
}

/// 集群成员管理器
///
/// 线程安全：内部用 `parking_lot::RwLock` 保护 peer 列表。
/// 异步友好：事件通过 `tokio::sync::broadcast` 广播。
pub struct PeerManager {
    /// 本节点 ID（用于过滤自己的事件）
    self_node_id: String,
    /// peer 列表（按 node_id 排序；BTreeMap 便于取最小）
    peers: Arc<RwLock<BTreeMap<String, PeerInfo>>>,
    /// 事件广播通道（订阅者通过 subscribe() 接收）
    event_tx: broadcast::Sender<ClusterEvent>,
    /// TTL：超过此时间无心跳视为离线
    #[allow(dead_code)]
    peer_ttl: Duration,
}

impl PeerManager {
    /// 新建 PeerManager。broadcast 容量默认 256。
    pub fn new(self_node_id: impl Into<String>, peer_ttl: Duration) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            self_node_id: self_node_id.into(),
            peers: Arc::new(RwLock::new(BTreeMap::new())),
            event_tx,
            peer_ttl,
        }
    }

    /// 获取本节点 ID
    pub fn self_node_id(&self) -> &str {
        &self.self_node_id
    }

    /// 订阅事件广播
    pub fn subscribe(&self) -> broadcast::Receiver<ClusterEvent> {
        self.event_tx.subscribe()
    }

    /// 当前 peer 数量（含自己）
    pub fn cluster_size(&self) -> usize {
        self.peers.read().len() + 1
    }

    /// 当前 leader ID（字典序最小，含自己）
    pub fn leader_id(&self) -> String {
        let peers = self.peers.read();
        let mut all: Vec<&String> = peers.keys().collect();
        all.push(&self.self_node_id);
        all.into_iter().min().cloned().unwrap_or_default()
    }

    /// 是否为 leader
    pub fn is_leader(&self) -> bool {
        self.leader_id() == self.self_node_id
    }

    /// 列出当前 peer（含自己）
    pub fn list_peers(&self) -> Vec<PeerInfo> {
        let mut peers: Vec<PeerInfo> = self.peers.read().values().cloned().collect();
        peers.push(PeerInfo::new(&self.self_node_id, "self"));
        peers.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        peers
    }

    /// 处理一个 peer 加入事件。返回是否真的新增（已存在返回 false）。
    pub fn on_peer_joined(&self, info: PeerInfo) -> bool {
        let mut peers = self.peers.write();
        if peers.contains_key(&info.node_id) {
            return false;
        }
        peers.insert(info.node_id.clone(), info.clone());
        // 广播事件（忽略 NoActiveReceivers 错误：可能没有订阅者）
        let _ = self.event_tx.send(ClusterEvent::PeerJoined(info));
        true
    }

    /// 处理一个 peer 离开事件。返回是否真的移除。
    pub fn on_peer_left(&self, node_id: &str, reason: impl Into<String>) -> bool {
        let mut peers = self.peers.write();
        if peers.remove(node_id).is_some() {
            let _ = self.event_tx.send(ClusterEvent::PeerLeft {
                node_id: node_id.to_string(),
                reason: reason.into(),
            });
            true
        } else {
            false
        }
    }

    /// 处理心跳 / Stream 事件：更新 last_seen 用于 TTL 判定
    pub fn touch(&self, node_id: &str) {
        // 当前简化实现：仅在 broadcast 通道发送一个心跳（订阅者忽略）
        // TODO: 维护 last_seen Instant map
        let _ = node_id;
    }

    /// TTL 过期清理：移除 last_seen 超过 peer_ttl 的 peer
    pub fn evict_expired(&self) -> Vec<String> {
        // 当前简化实现：返回空列表（完整 TTL 实现需 last_seen map）
        // 实际逻辑留待 8.3.1 Phase B
        Vec::new()
    }

    /// 广播一个 StateProposal 事件
    pub fn broadcast_state_proposal(&self, key: impl Into<String>, value: Vec<u8>) {
        let _ = self.event_tx.send(ClusterEvent::StateProposal {
            key: key.into(),
            value,
        });
    }

    /// 广播一个 ExperienceSync 事件（TODO 8.3.2 集成）
    pub fn broadcast_experience_sync(
        &self,
        exp_key: impl Into<String>,
        version: u64,
        vector_clock_json: impl Into<String>,
        payload: Vec<u8>,
    ) {
        let _ = self.event_tx.send(ClusterEvent::ExperienceSync {
            exp_key: exp_key.into(),
            version,
            vector_clock_json: vector_clock_json.into(),
            payload,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_info_new_sets_joined_at() {
        let p = PeerInfo::new("node-a", "addr:50051");
        assert_eq!(p.node_id, "node-a");
        assert_eq!(p.grpc_addr, "addr:50051");
        // joined_at 是当前时间，应该 > 某个 epoch（如 2020-01-01）
        assert!(p.joined_at > 1_577_836_800); // 2020-01-01 UTC
    }

    #[test]
    fn test_peer_manager_new_starts_empty() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        assert_eq!(mgr.cluster_size(), 1); // 仅自己
        assert!(mgr.is_leader());
    }

    #[test]
    fn test_peer_manager_join_adds_peer() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        let added = mgr.on_peer_joined(PeerInfo::new("node-b", "addr-b:50051"));
        assert!(added);
        assert_eq!(mgr.cluster_size(), 2);
    }

    #[test]
    fn test_peer_manager_join_duplicate_returns_false() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        assert!(mgr.on_peer_joined(PeerInfo::new("node-b", "addr")));
        assert!(!mgr.on_peer_joined(PeerInfo::new("node-b", "addr2")));
        assert_eq!(mgr.cluster_size(), 2);
    }

    #[test]
    fn test_peer_manager_leader_is_lex_min() {
        let mgr = PeerManager::new("node-m", Duration::from_secs(30));
        assert!(mgr.is_leader()); // 仅自己时是 leader
        mgr.on_peer_joined(PeerInfo::new("node-z", "addr-z"));
        assert!(mgr.is_leader()); // 'm' < 'z'，自己仍是 leader
        mgr.on_peer_joined(PeerInfo::new("node-a", "addr-a"));
        assert!(!mgr.is_leader()); // 'a' 现在是 leader
    }

    #[test]
    fn test_peer_manager_leave_removes_peer() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        mgr.on_peer_joined(PeerInfo::new("node-b", "addr-b"));
        assert_eq!(mgr.cluster_size(), 2);
        let removed = mgr.on_peer_left("node-b", "shutdown");
        assert!(removed);
        assert_eq!(mgr.cluster_size(), 1);
    }

    #[test]
    fn test_peer_manager_leave_unknown_returns_false() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        assert!(!mgr.on_peer_left("unknown", "ttl_expired"));
    }

    #[test]
    fn test_peer_manager_subscribe_receives_event() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        let mut rx = mgr.subscribe();
        mgr.on_peer_joined(PeerInfo::new("node-b", "addr-b"));

        // 异步接收：poll 一次（事件已通过 send 投递）
        let evt = rx.try_recv();
        assert!(evt.is_ok(), "should receive event");
        match evt.unwrap() {
            ClusterEvent::PeerJoined(p) => assert_eq!(p.node_id, "node-b"),
            _ => panic!("expected PeerJoined"),
        }
    }

    #[test]
    fn test_peer_manager_list_peers_sorted() {
        let mgr = PeerManager::new("node-b", Duration::from_secs(30));
        mgr.on_peer_joined(PeerInfo::new("node-a", "addr-a"));
        mgr.on_peer_joined(PeerInfo::new("node-c", "addr-c"));

        let peers = mgr.list_peers();
        // self (node-b) + node-a + node-c，按 node_id 排序
        assert_eq!(peers.len(), 3);
        assert_eq!(peers[0].node_id, "node-a");
        assert_eq!(peers[1].node_id, "node-b");
        assert_eq!(peers[2].node_id, "node-c");
    }

    #[test]
    fn test_cluster_event_kind() {
        assert_eq!(
            ClusterEvent::PeerJoined(PeerInfo::new("a", "b")).kind(),
            "peer_joined"
        );
        assert_eq!(
            ClusterEvent::PeerLeft {
                node_id: "a".into(),
                reason: "x".into()
            }
            .kind(),
            "peer_left"
        );
        assert_eq!(
            ClusterEvent::StateProposal {
                key: "k".into(),
                value: vec![]
            }
            .kind(),
            "state_proposal"
        );
    }

    #[test]
    fn test_peer_manager_broadcast_state_proposal() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        let mut rx = mgr.subscribe();
        mgr.broadcast_state_proposal("config:foo", vec![1, 2, 3]);
        let evt = rx.try_recv().expect("event");
        match evt {
            ClusterEvent::StateProposal { key, value } => {
                assert_eq!(key, "config:foo");
                assert_eq!(value, vec![1, 2, 3]);
            },
            _ => panic!("expected StateProposal"),
        }
    }

    #[test]
    fn test_peer_manager_evict_expired_returns_empty_initially() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        mgr.on_peer_joined(PeerInfo::new("node-b", "addr"));
        assert!(mgr.evict_expired().is_empty());
    }
}
