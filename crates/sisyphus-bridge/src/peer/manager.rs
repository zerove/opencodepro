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
    StateProposal {
        /// 本提案描述的是哪个节点的策略快照（v0.4.0 Phase G：定向转发用）
        source_node_id: String,
        key: String,
        value: Vec<u8>,
    },
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
///
/// TTL 机制：
/// - 每个 peer 维护 `last_seen: Instant`（on_peer_joined / touch 时刷新）
/// - `evict_expired()` 移除 last_seen 超过 peer_ttl 的 peer，并广播 PeerLeft
pub struct PeerManager {
    /// 本节点 ID（用于过滤自己的事件）
    self_node_id: String,
    /// peer 列表（按 node_id 排序；BTreeMap 便于取最小）
    peers: Arc<RwLock<BTreeMap<String, PeerInfo>>>,
    /// 事件广播通道（**本地产生**，需要转发给所有 peer）
    ///
    /// 拨号器 [`crate::peer::fallback::spawn_static_peers`] 与服务端
    /// [`crate::peer::service::ClusterServiceImpl::stream`] 订阅的是这一路。
    event_tx: broadcast::Sender<ClusterEvent>,
    /// 入站事件通道（**从远端收到**，只供本地消费，不再转发）
    ///
    /// 与 `event_tx` 分离是为了让「不把远端事件再转发回去」成为结构保证，
    /// 而不是靠接收端解析主键、自行比对 origin 的约定。之前两者共用一个通道：
    /// 入站事件重新进入 `event_tx` 后会被拨号器当作本地事件再发给所有 peer，
    /// 回弹靠 `ExperienceSyncHandler` 的自过滤终止——主键一旦不符合预期格式
    /// （`experience_origin_from_key` 返回 `None`）就拦不住，全连接集群下还会
    /// 出现 O(N²) 的重复投递。
    inbound_tx: broadcast::Sender<ClusterEvent>,
    /// TTL：超过此时间无心跳视为离线
    peer_ttl: Duration,
    /// 每个 peer 的最后心跳时间（与 peers 同步维护）
    last_seen: Arc<RwLock<BTreeMap<String, Instant>>>,
    /// 累计发出去的 `StateProposal` 事件数（v0.4.0 Phase F：转发放大量化）。
    ///
    /// 广播本身是 fan-out（一个事件 → 所有订阅者 / 每个 dialer 一条 outbound），
    /// 这里按「事件发出」计数，是观察 O(N²) 放大时「发送次数」的入口指标。
    /// 与 [`Self::broadcast_state_proposal`] 同处，天然对齐发送动作。
    state_proposal_broadcasts: std::sync::atomic::AtomicU64,
    /// Phase 6.2b：本地产生并发往本地广播/对端流的经验同步事件数。
    experience_sync_broadcasts: std::sync::atomic::AtomicU64,
    /// Phase 6.2b：从远端投递到入站总线的经验同步事件数（不含本地广播）。
    experience_sync_inbound_delivered: std::sync::atomic::AtomicU64,
    /// Phase 6.2b：入站经验被 CRDT 合并实际写入本地（Applied）的次数。
    experience_sync_inbound_applied: std::sync::atomic::AtomicU64,
    /// Phase 6.2b/6.3：入站经验被拒绝/丢弃的总次数（= deserialize + quality + apply）。
    /// Kept/Equal（幂等/本地更新）不计入 rejected，以免与“拒绝”语义混淆。
    experience_sync_inbound_rejected: std::sync::atomic::AtomicU64,
    experience_sync_inbound_rejected_deserialize_count: std::sync::atomic::AtomicU64,
    experience_sync_inbound_rejected_quality_count: std::sync::atomic::AtomicU64,
    experience_sync_inbound_rejected_apply_count: std::sync::atomic::AtomicU64,
}

impl PeerManager {
    /// 新建 PeerManager。broadcast 容量默认 256。
    pub fn new(self_node_id: impl Into<String>, peer_ttl: Duration) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let (inbound_tx, _) = broadcast::channel(256);
        Self {
            self_node_id: self_node_id.into(),
            peers: Arc::new(RwLock::new(BTreeMap::new())),
            event_tx,
            inbound_tx,
            peer_ttl,
            last_seen: Arc::new(RwLock::new(BTreeMap::new())),
            state_proposal_broadcasts: std::sync::atomic::AtomicU64::new(0),
            experience_sync_broadcasts: std::sync::atomic::AtomicU64::new(0),
            experience_sync_inbound_delivered: std::sync::atomic::AtomicU64::new(0),
            experience_sync_inbound_applied: std::sync::atomic::AtomicU64::new(0),
            experience_sync_inbound_rejected: std::sync::atomic::AtomicU64::new(0),
            experience_sync_inbound_rejected_deserialize_count: std::sync::atomic::AtomicU64::new(
                0,
            ),
            experience_sync_inbound_rejected_quality_count: std::sync::atomic::AtomicU64::new(0),
            experience_sync_inbound_rejected_apply_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 获取本节点 ID
    pub fn self_node_id(&self) -> &str {
        &self.self_node_id
    }

    /// 订阅**本地产生**的事件广播。
    ///
    /// 这条通道的事件会被拨号器转发给所有 peer，因此只应供「要把自己的事件
    /// 发出去」的组件订阅（拨号器、服务端 stream）。远端来的事件走
    /// [`PeerManager::subscribe_inbound`]。
    pub fn subscribe(&self) -> broadcast::Receiver<ClusterEvent> {
        self.event_tx.subscribe()
    }

    /// 订阅**入站**（从远端收到）的事件。
    ///
    /// 这条通道不会流向任何 peer：`ExperienceSyncHandler` 订阅它，把远程经验
    /// 合并进本地库；拨号器不订阅它，因此事件不会在集群里反弹。
    pub fn subscribe_inbound(&self) -> broadcast::Receiver<ClusterEvent> {
        self.inbound_tx.subscribe()
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
        let mut last_seen = self.last_seen.write();
        if peers.contains_key(&info.node_id) {
            // 已存在：刷新 last_seen（视为重连 / 心跳）
            last_seen.insert(info.node_id.clone(), Instant::now());
            return false;
        }
        peers.insert(info.node_id.clone(), info.clone());
        last_seen.insert(info.node_id.clone(), Instant::now());
        // 广播事件（忽略 NoActiveReceivers 错误：可能没有订阅者）
        let _ = self.event_tx.send(ClusterEvent::PeerJoined(info));
        true
    }

    /// 处理一个 peer 离开事件。返回是否真的移除。
    pub fn on_peer_left(&self, node_id: &str, reason: impl Into<String>) -> bool {
        let mut peers = self.peers.write();
        let mut last_seen = self.last_seen.write();
        if peers.remove(node_id).is_some() {
            last_seen.remove(node_id);
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
    pub fn touch(&self, node_id: &str) -> bool {
        let mut last_seen = self.last_seen.write();
        if last_seen.contains_key(node_id) {
            last_seen.insert(node_id.to_string(), Instant::now());
            true
        } else {
            false
        }
    }

    /// TTL 过期清理：移除 last_seen 超过 peer_ttl 的 peer，并广播 PeerLeft
    ///
    /// 返回被移除的 peer node_id 列表。
    pub fn evict_expired(&self) -> Vec<String> {
        let now = Instant::now();
        let mut expired = Vec::new();
        {
            // 第一遍：扫描 last_seen 找出过期节点
            let last_seen = self.last_seen.read();
            for (node_id, ts) in last_seen.iter() {
                if now.duration_since(*ts) > self.peer_ttl {
                    expired.push(node_id.clone());
                }
            }
        }
        // 第二遍：移除并广播（避免持锁期间发广播）
        let mut peers = self.peers.write();
        let mut last_seen = self.last_seen.write();
        let mut evicted = Vec::new();
        for node_id in &expired {
            if peers.remove(node_id).is_some() {
                last_seen.remove(node_id);
                let _ = self.event_tx.send(ClusterEvent::PeerLeft {
                    node_id: node_id.clone(),
                    reason: "ttl_expired".to_string(),
                });
                evicted.push(node_id.clone());
            }
        }
        evicted
    }

    /// 获取某 peer 的 last_seen（用于监控 / 测试）
    pub fn last_seen(&self, node_id: &str) -> Option<Instant> {
        self.last_seen.read().get(node_id).copied()
    }

    /// 广播一个 StateProposal 事件
    pub fn broadcast_state_proposal(
        &self,
        source_node_id: impl Into<String>,
        key: impl Into<String>,
        value: Vec<u8>,
    ) {
        self.state_proposal_broadcasts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = self.event_tx.send(ClusterEvent::StateProposal {
            source_node_id: source_node_id.into(),
            key: key.into(),
            value,
        });
    }

    /// 累计发出去的 `StateProposal` 事件数（v0.4.0 Phase F：转发放大量化）。
    ///
    /// 用于量化「全连接集群下每接受一份快照会产生多少次发送」：全连接时一个
    /// 快照从任一节点发出 = (N-1) 条 outbound；链式拓扑下经中间节点转发一次 =
    /// 多条。与 [`Self::broadcast_state_proposal`] 同处计数，避免双重统计。
    pub fn state_proposal_broadcasts(&self) -> u64 {
        self.state_proposal_broadcasts.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Phase 6.2b：累计本地产生的经验同步广播数。
    pub fn experience_sync_broadcasts(&self) -> u64 {
        self.experience_sync_broadcasts.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Phase 6.2b：累计投递到入站总线的经验同步事件数。
    pub fn experience_sync_inbound_delivered(&self) -> u64 {
        self.experience_sync_inbound_delivered.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Phase 6.2b：累计入站经验实际写入次数（CRDT Applied）。
    pub fn experience_sync_inbound_applied(&self) -> u64 {
        self.experience_sync_inbound_applied.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Phase 6.2b/6.3：累计入站经验拒绝总次数（= deserialize + quality + apply）。
    pub fn experience_sync_inbound_rejected(&self) -> u64 {
        self.experience_sync_inbound_rejected.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn experience_sync_inbound_rejected_deserialize(&self) -> u64 {
        self.experience_sync_inbound_rejected_deserialize_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn experience_sync_inbound_rejected_quality(&self) -> u64 {
        self.experience_sync_inbound_rejected_quality_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn experience_sync_inbound_rejected_apply(&self) -> u64 {
        self.experience_sync_inbound_rejected_apply_count.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Phase 6.2b：记录入站经验应用结果（由 handler 调用，不改变合并语义）。
    pub(crate) fn record_experience_sync_applied(&self) {
        self.experience_sync_inbound_applied.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn bump_rejected(&self, kind: &std::sync::atomic::AtomicU64) {
        kind.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.experience_sync_inbound_rejected.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn record_experience_sync_rejected_deserialize(&self) {
        self.bump_rejected(&self.experience_sync_inbound_rejected_deserialize_count);
    }

    pub(crate) fn record_experience_sync_rejected_quality(&self) {
        self.bump_rejected(&self.experience_sync_inbound_rejected_quality_count);
    }

    pub(crate) fn record_experience_sync_rejected_apply(&self) {
        self.bump_rejected(&self.experience_sync_inbound_rejected_apply_count);
    }

    /// 广播一个本地产生的 `ExperienceSync` 事件。
    ///
    /// 事件同时进入本地订阅者（`ExperienceSyncHandler`）与已建立的对端流，
    /// 由 `PeerBroadcastExperienceBank` 在本地落库成功后调用。
    pub fn broadcast_experience_sync(
        &self,
        exp_key: impl Into<String>,
        version: u64,
        vector_clock_json: impl Into<String>,
        payload: Vec<u8>,
    ) {
        self.experience_sync_broadcasts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = self.event_tx.send(ClusterEvent::ExperienceSync {
            exp_key: exp_key.into(),
            version,
            vector_clock_json: vector_clock_json.into(),
            payload,
        });
    }

    /// 把从远端收到的 `ExperienceSync` 投递到**入站**事件总线。
    ///
    /// 与 [`PeerManager::broadcast_experience_sync`] 的区别：后者是本地产生的
    /// 写入，会顺着已建立的流再发给其他 peer；本方法只写入 `inbound_tx`，
    /// 拨号器不订阅该通道，因此事件到此为止。若在此再走一次转发，事件会在
    /// 集群里无限反弹。
    ///
    /// 此前 `ClusterServiceImpl::apply_mgr_event` 对入站 `ExperienceSync` 直接
    /// 丢弃（标着「留待 8.3.2 集成」），导致跨节点经验同步整条链路不通。
    pub fn deliver_inbound_experience_sync(
        &self,
        exp_key: impl Into<String>,
        version: u64,
        vector_clock_json: impl Into<String>,
        payload: Vec<u8>,
    ) {
        self.experience_sync_inbound_delivered.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = self.inbound_tx.send(ClusterEvent::ExperienceSync {
            exp_key: exp_key.into(),
            version,
            vector_clock_json: vector_clock_json.into(),
            payload,
        });
    }

    /// 把从远端收到的 `StateProposal` 投递到**入站**事件总线（ADR-017）。
    ///
    /// 与 [`PeerManager::broadcast_state_proposal`] 的区别同上：本方法只写
    /// `inbound_tx`，拨号器不订阅该通道，因此不会回流到集群。
    ///
    /// 在此之前入站 `StateProposal` 在 `proto_to_mgr_event` 就被丢弃了（注释写
    /// 「handled separately」，实际没有消费方），于是任何依赖它传播的集群状态
    /// 都只出去不回来。
    pub fn deliver_inbound_state_proposal(
        &self,
        source_node_id: impl Into<String>,
        key: impl Into<String>,
        value: Vec<u8>,
    ) {
        let _ = self.inbound_tx.send(ClusterEvent::StateProposal {
            source_node_id: source_node_id.into(),
            key: key.into(),
            value,
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

    /// 回归锁定：入站事件不得进入本地广播通道。
    ///
    /// 两条通道合并成一条时，远端来的经验会重新进入 `event_tx`，被拨号器当成
    /// 本地事件再发给所有 peer；回弹只能靠接收端解析主键、比对 origin 的约定
    /// 终止，而主键格式一旦不符合预期（`experience_origin_from_key` 返回 `None`）
    /// 就拦不住，全连接集群下退化为 O(N²) 重复投递。
    #[test]
    fn test_inbound_experience_sync_stays_off_local_broadcast_channel() {
        let mgr = PeerManager::new("node-b", Duration::from_secs(30));
        let mut local = mgr.subscribe();
        let mut inbound = mgr.subscribe_inbound();

        // 模拟对端发来的经验
        mgr.deliver_inbound_experience_sync("exp:1:node-a:0", 1, "{}", vec![1]);

        assert!(inbound.try_recv().is_ok(), "入站订阅者应收到远程经验");
        assert!(
            local.try_recv().is_err(),
            "本地广播通道不应看到入站事件，否则会被拨号器转发回集群"
        );
    }

    /// 反向断言：本地产生的写入只走本地广播通道。
    ///
    /// `ExperienceSyncHandler` 订阅的是入站通道，本地经验已经由
    /// `PeerBroadcastExperienceBank` 直接落库，不应该再经 CRDT 合并走一遍。
    #[test]
    fn test_local_broadcast_is_not_visible_on_inbound_channel() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        let mut inbound = mgr.subscribe_inbound();
        let mut local = mgr.subscribe();

        mgr.broadcast_experience_sync("exp:1:node-a:0", 1, "{}", vec![1]);

        assert!(local.try_recv().is_ok(), "本地订阅者应收到广播");
        assert!(inbound.try_recv().is_err(), "入站通道不应收到本地广播");
    }

    /// 本地广播必须仍能到达拨号器订阅的通道（分离两路不能误伤出站）。
    #[test]
    fn test_local_broadcast_reaches_dialer_channel() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        let mut dialer = mgr.subscribe();
        mgr.broadcast_experience_sync("exp:1:node-a:7", 1, "{}", vec![1]);

        match dialer.try_recv() {
            Ok(ClusterEvent::ExperienceSync { exp_key, .. }) => {
                assert_eq!(exp_key, "exp:1:node-a:7");
            },
            other => panic!("拨号器通道应收到本地广播，实际：{other:?}"),
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
                source_node_id: "node-b".into(),
                key: "k".into(),
                value: vec![]
            }
            .kind(),
            "state_proposal"
        );
    }

    #[test]
    fn test_peer_manager_state_proposal_broadcast_counter() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        assert_eq!(mgr.state_proposal_broadcasts(), 0, "初始计数为 0");

        mgr.broadcast_state_proposal("node-a", "sisyphus.strategy-weights/v1", vec![1]);
        mgr.broadcast_state_proposal("node-a", "sisyphus.strategy-weights/v1", vec![2]);
        assert_eq!(mgr.state_proposal_broadcasts(), 2);

        // 入站通道不计数：它只走 inbound_tx，不构成「发出」。
        mgr.deliver_inbound_state_proposal("node-a", "sisyphus.strategy-weights/v1", vec![3]);
        assert_eq!(mgr.state_proposal_broadcasts(), 2);
    }

    #[test]
    fn test_peer_manager_broadcast_state_proposal() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        let mut rx = mgr.subscribe();
        mgr.broadcast_state_proposal("node-a", "config:foo", vec![1, 2, 3]);
        let evt = rx.try_recv().expect("event");
        match evt {
            ClusterEvent::StateProposal {
                source_node_id,
                key,
                value,
            } => {
                assert_eq!(source_node_id, "node-a");
                assert_eq!(key, "config:foo");
                assert_eq!(value, vec![1, 2, 3]);
            },
            _ => panic!("expected StateProposal"),
        }
    }

    #[test]
    fn test_peer_manager_evict_expired_no_expired_within_ttl() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        mgr.on_peer_joined(PeerInfo::new("node-b", "addr"));
        // 新加入的 peer 立即调用 evict_expired：last_seen = now，TTL=30s，未过期
        assert!(mgr.evict_expired().is_empty());
        assert_eq!(mgr.cluster_size(), 2);
    }

    #[test]
    fn test_peer_manager_touch_updates_last_seen() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        mgr.on_peer_joined(PeerInfo::new("node-b", "addr"));
        // touch 存在的 peer 应返回 true
        assert!(mgr.touch("node-b"));
        // touch 不存在的 peer 返回 false
        assert!(!mgr.touch("unknown"));
        // last_seen 应被设置
        assert!(mgr.last_seen("node-b").is_some());
    }

    #[test]
    fn test_peer_manager_evict_expired_removes_old_peers() {
        // 用极短 TTL（10ms）让 last_seen 立即过期
        let mgr = PeerManager::new("node-a", Duration::from_millis(10));
        mgr.on_peer_joined(PeerInfo::new("node-b", "addr"));
        assert_eq!(mgr.cluster_size(), 2);
        // 睡 50ms 让 TTL 过期
        std::thread::sleep(Duration::from_millis(50));
        let evicted = mgr.evict_expired();
        assert_eq!(evicted, vec!["node-b".to_string()]);
        assert_eq!(mgr.cluster_size(), 1);
        // 广播事件：再次调用 evict_expired 应无更多事件
        assert!(mgr.evict_expired().is_empty());
    }

    #[test]
    fn test_peer_manager_touch_prevents_eviction() {
        let mgr = PeerManager::new("node-a", Duration::from_millis(50));
        mgr.on_peer_joined(PeerInfo::new("node-b", "addr"));
        // 30ms 后 touch 刷新 last_seen
        std::thread::sleep(Duration::from_millis(30));
        mgr.touch("node-b");
        // 再等 30ms（总计 60ms，但 last_seen 只有 30ms），不应过期
        std::thread::sleep(Duration::from_millis(30));
        assert!(mgr.evict_expired().is_empty());
        assert_eq!(mgr.cluster_size(), 2);
    }

    #[test]
    fn test_peer_manager_on_peer_left_clears_last_seen() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        mgr.on_peer_joined(PeerInfo::new("node-b", "addr"));
        assert!(mgr.last_seen("node-b").is_some());
        mgr.on_peer_left("node-b", "shutdown");
        assert!(mgr.last_seen("node-b").is_none());
        // 即使 TTL 过期也不会再处理这个 node_id
        std::thread::sleep(Duration::from_millis(50));
        assert!(mgr.evict_expired().is_empty());
    }

    /// Phase 6.2b：经验同步计数对齐 state_proposal_broadcasts；入站投递不计入 broadcast。
    #[test]
    fn test_experience_sync_counters_separate_broadcast_and_inbound() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        assert_eq!(mgr.experience_sync_broadcasts(), 0);
        assert_eq!(mgr.experience_sync_inbound_delivered(), 0);
        assert_eq!(mgr.experience_sync_inbound_applied(), 0);
        assert_eq!(mgr.experience_sync_inbound_rejected(), 0);

        mgr.broadcast_experience_sync("exp:1:node-a:0", 1, "{}", vec![1]);
        mgr.broadcast_experience_sync("exp:1:node-a:1", 2, "{}", vec![2]);
        assert_eq!(mgr.experience_sync_broadcasts(), 2);
        assert_eq!(mgr.experience_sync_inbound_delivered(), 0);

        mgr.deliver_inbound_experience_sync("exp:1:node-b:0", 1, "{}", vec![3]);
        assert_eq!(
            mgr.experience_sync_broadcasts(),
            2,
            "入站投递不应增加 broadcast"
        );
        assert_eq!(mgr.experience_sync_inbound_delivered(), 1);

        mgr.record_experience_sync_applied();
        mgr.record_experience_sync_rejected_deserialize();
        mgr.record_experience_sync_rejected_quality();
        mgr.record_experience_sync_rejected_apply();
        assert_eq!(mgr.experience_sync_inbound_applied(), 1);
        assert_eq!(mgr.experience_sync_inbound_rejected(), 3);
        assert_eq!(mgr.experience_sync_inbound_rejected_deserialize(), 1);
        assert_eq!(mgr.experience_sync_inbound_rejected_quality(), 1);
        assert_eq!(mgr.experience_sync_inbound_rejected_apply(), 1);
    }
}
