//! ADR-019 控制面：分布式任务租约（v1，默认门禁关闭）。
//!
//! 触发用例（用户确认“按默认实施”）：`lease.acquire/renew/release`，`request_id` 幂等，
//! fencing `generation` 单调递增，TTL 过期。多数派提交由 `ClusterConsensus` +
//! `PeerProposer` 承载；超时/不确定 = 显式错误，不判定成功。
//!
//! 限制（v1）：非 Raft/线性一致日志；无自动续约、跨集群、动态成员变更共识。
//! 生产提交路径必须受 `[cluster.control_plane].enabled` 门禁，且满足 ADR-019 准入条件。

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseRecord {
    pub key: String,
    pub owner: String,
    pub generation: u64,
    pub expires_at_unix_ms: u64,
    pub last_request_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeasePayload {
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub ttl_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlDecision {
    pub accepted: bool,
    pub applied_generation: u64,
    pub current_generation: u64,
    pub error_code: String,
    pub error_message: String,
    pub idempotent_replay: bool,
    pub lease: Option<LeaseRecord>,
}

impl ControlDecision {
    fn ok(generation: u64, lease: LeaseRecord, idempotent_replay: bool) -> Self {
        Self {
            accepted: true,
            applied_generation: generation,
            current_generation: generation,
            error_code: String::new(),
            error_message: String::new(),
            idempotent_replay,
            lease: Some(lease),
        }
    }
    fn err(code: &str, msg: impl Into<String>, current_generation: u64) -> Self {
        Self {
            accepted: false,
            applied_generation: 0,
            current_generation,
            error_code: code.to_string(),
            error_message: msg.into(),
            idempotent_replay: false,
            lease: None,
        }
    }
}

#[derive(Clone, Debug)]
struct LeaseEntry {
    record: LeaseRecord,
    expires_at: Instant,
    /// request_id -> 已完成请求的结果快照（幂等重放）。
    ///
    /// 条目在释放/过期后**不删除**：`record.generation` 作为该 key 的 fencing 高水位，
    /// 保证「释放/过期后重新获取」的 generation 单调递增；`responses` 也随之保留，
    /// 使 `lease.release` 等请求在成功后被重试时仍返回同一结果而非 `LEASE_NOT_FOUND`。
    responses: HashMap<String, CachedResponse>,
}

/// 已完成请求的快照（用于 `request_id` 幂等重放）。
///
/// 保存 `record` 快照而非读取当前 entry：同一 request_id 的旧 acquire 在新一轮
/// 获取之后被重放时，必须返回当时的 owner/generation，不能把新租约的信息
/// 冒充成旧请求的结果。
#[derive(Clone, Debug)]
struct CachedResponse {
    generation: u64,
    accepted: bool,
    error_code: String,
    error_message: String,
    record: Option<LeaseRecord>,
}

impl CachedResponse {
    fn accepted(record: &LeaseRecord) -> Self {
        Self {
            generation: record.generation,
            accepted: true,
            error_code: String::new(),
            error_message: String::new(),
            record: Some(record.clone()),
        }
    }

    fn rejected(generation: u64, code: &str, message: &str) -> Self {
        Self {
            generation,
            accepted: false,
            error_code: code.to_string(),
            error_message: message.to_string(),
            record: None,
        }
    }
}

/// ADR-019 控制面只读观测计数（原子，不影响决策路径）。
#[derive(Debug, Default)]
pub struct ControlPlaneStats {
    pub submit_total: AtomicU64,
    pub submit_accepted: AtomicU64,
    pub submit_rejected: AtomicU64,
    pub submit_uncertain: AtomicU64,
    pub submit_disabled: AtomicU64,
    pub submit_unavailable: AtomicU64,
}

impl ControlPlaneStats {
    pub fn snapshot(&self) -> ControlPlaneStatsSnapshot {
        ControlPlaneStatsSnapshot {
            submit_total: self.submit_total.load(Ordering::Relaxed),
            submit_accepted: self.submit_accepted.load(Ordering::Relaxed),
            submit_rejected: self.submit_rejected.load(Ordering::Relaxed),
            submit_uncertain: self.submit_uncertain.load(Ordering::Relaxed),
            submit_disabled: self.submit_disabled.load(Ordering::Relaxed),
            submit_unavailable: self.submit_unavailable.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlPlaneStatsSnapshot {
    pub submit_total: u64,
    pub submit_accepted: u64,
    pub submit_rejected: u64,
    pub submit_uncertain: u64,
    pub submit_disabled: u64,
    pub submit_unavailable: u64,
}

pub struct ControlPlaneState {
    leases: RwLock<HashMap<String, LeaseEntry>>,
    now_unix_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    stats: ControlPlaneStats,
}

impl Default for ControlPlaneState {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlPlaneState {
    pub fn new() -> Self {
        Self {
            leases: RwLock::new(HashMap::new()),
            now_unix_ms: Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            }),
            stats: ControlPlaneStats::default(),
        }
    }

    #[cfg(test)]
    pub fn with_fixed_now(now_ms: u64) -> Self {
        Self {
            leases: RwLock::new(HashMap::new()),
            now_unix_ms: Box::new(move || now_ms),
            stats: ControlPlaneStats::default(),
        }
    }

    pub fn stats_snapshot(&self) -> ControlPlaneStatsSnapshot {
        self.stats.snapshot()
    }

    pub fn record_disabled(&self) {
        self.stats.submit_disabled.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_unavailable(&self) {
        self.stats.submit_unavailable.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_submit_outcome(&self, out: &LeaseQuorumOutcome) {
        self.stats.submit_total.fetch_add(1, Ordering::Relaxed);
        if out.uncertain {
            self.stats.submit_uncertain.fetch_add(1, Ordering::Relaxed);
        } else if out.decision.accepted {
            self.stats.submit_accepted.fetch_add(1, Ordering::Relaxed);
        } else {
            self.stats.submit_rejected.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn now_ms(&self) -> u64 {
        (self.now_unix_ms)()
    }

    fn is_expired(entry: &LeaseEntry, now: Instant) -> bool {
        now >= entry.expires_at
    }

    pub fn get(&self, key: &str) -> Option<LeaseRecord> {
        let now = Instant::now();
        let g = self.leases.read();
        match g.get(key) {
            // 过期/已释放条目保留为 generation 高水位与幂等快照，对外不可见。
            Some(e) if !Self::is_expired(e, now) => Some(e.record.clone()),
            _ => None,
        }
    }

    /// 本地应用控制提案（幂等）。由 `ProposeControl` RPC handler 调用。
    /// `min_ttl_secs`/`max_ttl_secs` 由调用方按配置传入；非法参数拒绝。
    pub fn apply(
        &self,
        request_id: &str,
        key: &str,
        operation: &str,
        payload_json: &[u8],
        expected_generation: u64,
        min_ttl_secs: u64,
        max_ttl_secs: u64,
    ) -> ControlDecision {
        if request_id.trim().is_empty() || key.trim().is_empty() {
            return ControlDecision::err("INVALID_REQUEST", "request_id/key must not be empty", 0);
        }
        let now = Instant::now();
        let now_ms = self.now_ms();
        let mut g = self.leases.write();

        // 幂等重放（按 key）。已释放/过期的条目保留 generation 高水位与结果快照，
        // 因此这里不区分 active/expired：重复的 request_id 必须返回同一结果。
        if let Some(e) = g.get(key) {
            if let Some(c) = e.responses.get(request_id).cloned() {
                let mut d = if c.accepted {
                    let rec = c.record.clone().unwrap_or_else(|| e.record.clone());
                    ControlDecision::ok(c.generation, rec, true)
                } else {
                    let mut d = ControlDecision::err(&c.error_code, c.error_message, c.generation);
                    d.idempotent_replay = true;
                    d
                };
                d.current_generation = e.record.generation;
                return d;
            }
        }

        let payload: LeasePayload = match serde_json::from_slice(payload_json) {
            Ok(p) => p,
            Err(e) => {
                return ControlDecision::err(
                    "INVALID_PAYLOAD",
                    format!("invalid payload_json: {e}"),
                    0,
                )
            },
        };

        match operation {
            "lease.acquire" => {
                let ttl = payload.ttl_secs;
                if payload.owner.trim().is_empty() {
                    return ControlDecision::err("INVALID_PAYLOAD", "owner must not be empty", 0);
                }
                if ttl < min_ttl_secs || ttl > max_ttl_secs {
                    return ControlDecision::err(
                        "INVALID_TTL",
                        format!("ttl_secs must be in [{min_ttl_secs},{max_ttl_secs}]"),
                        g.get(key).map(|e| e.record.generation).unwrap_or(0),
                    );
                }
                // 仅 active 条目构成持有；已释放/过期条目的 generation 作为高水位。
                if let Some(e) = g.get_mut(key) {
                    if !Self::is_expired(e, now) {
                        let cur = e.record.generation;
                        let d = ControlDecision::err(
                            "LEASE_HELD",
                            format!("lease held by {}", e.record.owner),
                            cur,
                        );
                        e.responses.insert(
                            request_id.to_string(),
                            CachedResponse::rejected(cur, &d.error_code, &d.error_message),
                        );
                        return d;
                    }
                }
                let generation = g.get(key).map(|e| e.record.generation).unwrap_or(0) + 1;
                let record = LeaseRecord {
                    key: key.to_string(),
                    owner: payload.owner.clone(),
                    generation,
                    expires_at_unix_ms: now_ms.saturating_add(ttl.saturating_mul(1000)),
                    last_request_id: request_id.to_string(),
                };
                if let Some(e) = g.get_mut(key) {
                    // 复用过期/已释放条目：保留历史 request_id 快照，generation 继续递增。
                    e.record = record.clone();
                    e.expires_at = now + Duration::from_secs(ttl);
                    e.responses.insert(request_id.to_string(), CachedResponse::accepted(&record));
                } else {
                    let mut responses = HashMap::new();
                    responses.insert(request_id.to_string(), CachedResponse::accepted(&record));
                    g.insert(
                        key.to_string(),
                        LeaseEntry {
                            record: record.clone(),
                            expires_at: now + Duration::from_secs(ttl),
                            responses,
                        },
                    );
                }
                ControlDecision::ok(generation, record, false)
            },
            "lease.renew" | "lease.release" => {
                let Some(e) = g.get_mut(key) else {
                    return ControlDecision::err(
                        "LEASE_NOT_FOUND",
                        "lease not found or expired",
                        0,
                    );
                };
                if Self::is_expired(e, now) {
                    // 过期条目不删除：保留 generation 高水位与幂等快照。
                    return ControlDecision::err(
                        "LEASE_NOT_FOUND",
                        "lease expired",
                        e.record.generation,
                    );
                }
                if expected_generation != 0 && expected_generation != e.record.generation {
                    let cur = e.record.generation;
                    let d = ControlDecision::err(
                        "FENCING_MISMATCH",
                        format!("expected generation {expected_generation}, current {cur}"),
                        cur,
                    );
                    e.responses.insert(
                        request_id.to_string(),
                        CachedResponse::rejected(cur, &d.error_code, &d.error_message),
                    );
                    return d;
                }
                if !payload.owner.is_empty() && payload.owner != e.record.owner {
                    let cur = e.record.generation;
                    let d = ControlDecision::err(
                        "OWNER_MISMATCH",
                        format!("owner mismatch: held by {}", e.record.owner),
                        cur,
                    );
                    e.responses.insert(
                        request_id.to_string(),
                        CachedResponse::rejected(cur, &d.error_code, &d.error_message),
                    );
                    return d;
                }
                if operation == "lease.release" {
                    let cur = e.record.generation;
                    let mut rec = e.record.clone();
                    rec.last_request_id = request_id.to_string();
                    e.record = rec.clone();
                    // tombstone：立即过期，但保留 entry（generation 高水位 + 幂等快照）。
                    e.expires_at = now;
                    e.responses.insert(request_id.to_string(), CachedResponse::accepted(&rec));
                    return ControlDecision {
                        accepted: true,
                        applied_generation: cur,
                        current_generation: cur,
                        error_code: String::new(),
                        error_message: String::new(),
                        idempotent_replay: false,
                        lease: Some(rec),
                    };
                }
                // renew
                let ttl = if payload.ttl_secs == 0 {
                    30
                } else {
                    payload.ttl_secs
                };
                if ttl < min_ttl_secs || ttl > max_ttl_secs {
                    let cur = e.record.generation;
                    let d = ControlDecision::err(
                        "INVALID_TTL",
                        format!("ttl_secs must be in [{min_ttl_secs},{max_ttl_secs}]"),
                        cur,
                    );
                    e.responses.insert(
                        request_id.to_string(),
                        CachedResponse::rejected(cur, &d.error_code, &d.error_message),
                    );
                    return d;
                }
                e.record.generation += 1;
                e.record.expires_at_unix_ms = now_ms.saturating_add(ttl.saturating_mul(1000));
                e.record.last_request_id = request_id.to_string();
                e.expires_at = now + Duration::from_secs(ttl);
                let gen = e.record.generation;
                let rec = e.record.clone();
                e.responses.insert(request_id.to_string(), CachedResponse::accepted(&rec));
                ControlDecision::ok(gen, rec, false)
            },
            _ => ControlDecision::err(
                "UNSUPPORTED_OPERATION",
                format!("unsupported operation: {operation}"),
                0,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(owner: &str, ttl: u64) -> Vec<u8> {
        serde_json::to_vec(&LeasePayload {
            owner: owner.into(),
            ttl_secs: ttl,
        })
        .unwrap()
    }

    #[test]
    fn acquire_renew_release_and_fencing() {
        let s = ControlPlaneState::with_fixed_now(1_000);
        let d = s.apply(
            "r1",
            "lease:a",
            "lease.acquire",
            &payload("n1", 30),
            0,
            1,
            300,
        );
        assert!(d.accepted);
        assert_eq!(d.applied_generation, 1);
        let d2 = s.apply(
            "r2",
            "lease:a",
            "lease.renew",
            &payload("n1", 30),
            1,
            1,
            300,
        );
        assert!(d2.accepted);
        assert_eq!(d2.applied_generation, 2);
        let bad = s.apply(
            "r3",
            "lease:a",
            "lease.renew",
            &payload("n1", 30),
            1,
            1,
            300,
        );
        assert!(!bad.accepted);
        assert_eq!(bad.error_code, "FENCING_MISMATCH");
        let rel = s.apply(
            "r4",
            "lease:a",
            "lease.release",
            &payload("n1", 0),
            2,
            1,
            300,
        );
        assert!(rel.accepted);
        assert!(s.get("lease:a").is_none());
    }

    #[test]
    fn acquire_rejected_when_held() {
        let s = ControlPlaneState::with_fixed_now(1_000);
        assert!(s.apply("r1", "k", "lease.acquire", &payload("n1", 30), 0, 1, 300).accepted);
        let d = s.apply("r2", "k", "lease.acquire", &payload("n2", 30), 0, 1, 300);
        assert!(!d.accepted);
        assert_eq!(d.error_code, "LEASE_HELD");
    }

    #[test]
    fn request_id_idempotent_replay() {
        let s = ControlPlaneState::with_fixed_now(1_000);
        let a = s.apply("r1", "k", "lease.acquire", &payload("n1", 30), 0, 1, 300);
        assert!(a.accepted && !a.idempotent_replay);
        let b = s.apply("r1", "k", "lease.acquire", &payload("n1", 30), 0, 1, 300);
        assert!(b.accepted && b.idempotent_replay);
        assert_eq!(a.applied_generation, b.applied_generation);
        // different request while held -> rejected
        let c = s.apply("r2", "k", "lease.acquire", &payload("n2", 30), 0, 1, 300);
        assert!(!c.accepted);
    }

    /// ADR-019 fencing：释放后重新获取必须继续递增 generation，不能回退。
    #[test]
    fn generation_monotonic_across_release() {
        let s = ControlPlaneState::with_fixed_now(1_000);
        let a = s.apply(
            "r1",
            "lease:f",
            "lease.acquire",
            &payload("n1", 30),
            0,
            1,
            300,
        );
        assert!(a.accepted);
        assert_eq!(a.applied_generation, 1);
        let rel = s.apply(
            "r2",
            "lease:f",
            "lease.release",
            &payload("n1", 0),
            1,
            1,
            300,
        );
        assert!(rel.accepted);
        assert_eq!(rel.applied_generation, 1);
        assert!(s.get("lease:f").is_none());
        let b = s.apply(
            "r3",
            "lease:f",
            "lease.acquire",
            &payload("n2", 30),
            0,
            1,
            300,
        );
        assert!(b.accepted);
        assert_eq!(b.applied_generation, 2, "fencing generation 必须单调递增");
    }

    /// ADR-019 fencing：TTL 过期后重新获取同样继续递增（读操作不得丢弃高水位）。
    #[test]
    fn generation_monotonic_across_expiry() {
        let s = ControlPlaneState::with_fixed_now(1_000);
        assert!(
            s.apply(
                "r1",
                "lease:e",
                "lease.acquire",
                &payload("n1", 1),
                0,
                1,
                300
            )
            .accepted
        );
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(s.get("lease:e").is_none(), "过期租约对外不可见");
        let b = s.apply(
            "r2",
            "lease:e",
            "lease.acquire",
            &payload("n2", 30),
            0,
            1,
            300,
        );
        assert!(b.accepted);
        assert_eq!(b.applied_generation, 2, "过期后重新获取必须继续递增");
    }

    /// ADR-019 幂等：release 成功后的重试必须返回同一结果，而不是 LEASE_NOT_FOUND。
    #[test]
    fn release_replay_is_idempotent() {
        let s = ControlPlaneState::with_fixed_now(1_000);
        assert!(
            s.apply(
                "r1",
                "lease:r",
                "lease.acquire",
                &payload("n1", 30),
                0,
                1,
                300
            )
            .accepted
        );
        let rel = s.apply(
            "r2",
            "lease:r",
            "lease.release",
            &payload("n1", 0),
            1,
            1,
            300,
        );
        assert!(rel.accepted);
        let replay = s.apply(
            "r2",
            "lease:r",
            "lease.release",
            &payload("n1", 0),
            1,
            1,
            300,
        );
        assert!(replay.accepted, "{replay:?}");
        assert!(replay.idempotent_replay);
        assert_eq!(replay.applied_generation, rel.applied_generation);
    }

    /// ADR-019 幂等：旧 acquire 在新一轮获取后重放，必须返回当时的 owner/generation 快照。
    #[test]
    fn replay_after_reacquire_keeps_snapshot() {
        let s = ControlPlaneState::with_fixed_now(1_000);
        let first = s.apply(
            "r1",
            "lease:s",
            "lease.acquire",
            &payload("n1", 30),
            0,
            1,
            300,
        );
        assert!(first.accepted);
        assert!(
            s.apply(
                "r2",
                "lease:s",
                "lease.release",
                &payload("n1", 0),
                1,
                1,
                300
            )
            .accepted
        );
        let second = s.apply(
            "r3",
            "lease:s",
            "lease.acquire",
            &payload("n2", 30),
            0,
            1,
            300,
        );
        assert!(second.accepted);
        assert_eq!(second.applied_generation, 2);
        let replay = s.apply(
            "r1",
            "lease:s",
            "lease.acquire",
            &payload("n1", 30),
            0,
            1,
            300,
        );
        assert!(replay.accepted && replay.idempotent_replay);
        assert_eq!(replay.applied_generation, 1);
        let rec = replay.lease.expect("accepted replay carries snapshot");
        assert_eq!(rec.owner, "n1", "重放不得返回新租约的 owner");
        assert_eq!(rec.generation, 1);
        // 重放不得产生副作用
        let cur = s.get("lease:s").expect("current lease");
        assert_eq!(cur.owner, "n2");
        assert_eq!(cur.generation, 2);
    }

    /// ADR-019 fencing：释放后重新获取，旧 generation 的续约必须被拒绝。
    #[test]
    fn stale_fencing_generation_rejected_after_reacquire() {
        let s = ControlPlaneState::with_fixed_now(1_000);
        assert!(
            s.apply(
                "r1",
                "lease:t",
                "lease.acquire",
                &payload("n1", 30),
                0,
                1,
                300
            )
            .accepted
        );
        assert!(
            s.apply(
                "r2",
                "lease:t",
                "lease.release",
                &payload("n1", 0),
                1,
                1,
                300
            )
            .accepted
        );
        assert!(
            s.apply(
                "r3",
                "lease:t",
                "lease.acquire",
                &payload("n2", 30),
                0,
                1,
                300
            )
            .accepted
        );
        let stale = s.apply(
            "r4",
            "lease:t",
            "lease.renew",
            &payload("n1", 30),
            1,
            1,
            300,
        );
        assert!(!stale.accepted, "{stale:?}");
        assert_eq!(stale.error_code, "FENCING_MISMATCH");
    }
}

use crate::peer::manager::PeerManager;
use sisyphus_bridge_api::cluster_proto::{cluster_client::ClusterClient, ControlProposeRequest};
use tonic::transport::{Channel, ClientTlsConfig};

/// 6.5 租约多数派协调结果（v1：多数派 ack；超时/不确定=错误，不判定成功）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseQuorumOutcome {
    pub decision: ControlDecision,
    /// 达成 accepted 的节点数（含本地，若本地 apply 计入）。
    pub accepts: usize,
    /// 参与判定的集群规模（含 self）。
    pub cluster_size: usize,
    /// 是否因超时/连接失败等产生不确定性（此时不应判定成功）。
    pub uncertain: bool,
}

pub struct ControlPlaneCoordinator {
    peer_manager: Arc<PeerManager>,
    state: Arc<ControlPlaneState>,
    propose_timeout: Duration,
    connect_timeout: Duration,
    client_tls: Option<ClientTlsConfig>,
    /// ADR-019：协调节点本地 `(key, request_id)` 幂等/重试缓存（有界，缓解不确定提交重试风暴）。
    /// 跨节点幂等仍依赖各节点 `ControlPlaneState`；此缓存仅用于同一协调入口的快速重放。
    decision_cache: RwLock<std::collections::VecDeque<((String, String), LeaseQuorumOutcome)>>,
    /// gRPC channel 复用（按 endpoint）。失败时 best-effort 驱逐。
    channels: Arc<RwLock<HashMap<String, Channel>>>,
}

impl ControlPlaneCoordinator {
    const DECISION_CACHE_CAP: usize = 1024;

    pub fn new(
        peer_manager: Arc<PeerManager>,
        state: Arc<ControlPlaneState>,
        propose_timeout: Duration,
    ) -> Self {
        Self::with_timeouts(
            peer_manager,
            state,
            propose_timeout,
            propose_timeout.min(Duration::from_millis(1000)),
        )
    }

    pub fn with_timeouts(
        peer_manager: Arc<PeerManager>,
        state: Arc<ControlPlaneState>,
        propose_timeout: Duration,
        connect_timeout: Duration,
    ) -> Self {
        Self::with_timeouts_and_tls(peer_manager, state, propose_timeout, connect_timeout, None)
    }

    pub fn with_timeouts_and_tls(
        peer_manager: Arc<PeerManager>,
        state: Arc<ControlPlaneState>,
        propose_timeout: Duration,
        connect_timeout: Duration,
        client_tls: Option<ClientTlsConfig>,
    ) -> Self {
        Self {
            peer_manager,
            state,
            propose_timeout,
            connect_timeout,
            client_tls,
            decision_cache: RwLock::new(std::collections::VecDeque::new()),
            channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn state(&self) -> &Arc<ControlPlaneState> {
        &self.state
    }

    /// 通过多数派提交租约操作。`self` 先本地 apply（计入票数），再并发请求其他 peers。
    /// 任一导致多数派不确定的超时/错误 => `uncertain=true` 且不返回成功。
    fn cached_decision(&self, key: &str, request_id: &str) -> Option<LeaseQuorumOutcome> {
        let g = self.decision_cache.read();
        g.iter().find(|((k, r), _)| k == key && r == request_id).map(|(_, v)| v.clone())
    }

    fn cache_decision(&self, key: &str, request_id: &str, out: &LeaseQuorumOutcome) {
        let mut g = self.decision_cache.write();
        if g.iter().any(|((k, r), _)| k == key && r == request_id) {
            return;
        }
        if g.len() >= Self::DECISION_CACHE_CAP {
            g.pop_front();
        }
        g.push_back(((key.to_string(), request_id.to_string()), out.clone()));
    }

    pub async fn propose_lease(
        &self,
        request_id: &str,
        key: &str,
        operation: &str,
        payload_json: &[u8],
        expected_generation: u64,
        min_ttl_secs: u64,
        max_ttl_secs: u64,
    ) -> LeaseQuorumOutcome {
        if let Some(cached) = self.cached_decision(key, request_id) {
            let mut d = cached.decision;
            d.idempotent_replay = true;
            let out = LeaseQuorumOutcome {
                decision: d,
                ..cached
            };
            self.state.record_submit_outcome(&out);
            return out;
        }
        let cluster_size = self.peer_manager.cluster_size().max(1);
        let majority = cluster_size / 2 + 1;
        let local = self.state.apply(
            request_id,
            key,
            operation,
            payload_json,
            expected_generation,
            min_ttl_secs,
            max_ttl_secs,
        );
        // 本地拒绝（持有中/无效参数/fencing 不匹配等）不广播：被拒绝的请求绝不能
        // 修改 peer 状态（v1 无回滚，广播只会制造「协调节点拒绝但 peer 已应用」的分歧）。
        if !local.accepted {
            let out = LeaseQuorumOutcome {
                decision: local,
                accepts: 0,
                cluster_size,
                uncertain: false,
            };
            self.cache_decision(key, request_id, &out);
            self.state.record_submit_outcome(&out);
            return out;
        }
        // idempotent replay of accepted local decision is fine to count
        let mut accepts = usize::from(local.accepted);
        let mut uncertain = false;
        let local_decision = local.clone();

        let self_id = self.peer_manager.self_node_id().to_string();
        let peers: Vec<_> = self
            .peer_manager
            .list_peers()
            .into_iter()
            .filter(|p| {
                p.node_id != self_id && p.grpc_addr != "self" && !p.grpc_addr.trim().is_empty()
            })
            .collect();

        let req = ControlProposeRequest {
            request_id: request_id.to_string(),
            key: key.to_string(),
            operation: operation.to_string(),
            payload_json: payload_json.to_vec(),
            source_node_id: self_id.clone(),
            expected_generation,
        };

        let channels = self.channels.clone();
        let client_tls = self.client_tls.clone();
        let mut tasks = Vec::new();
        for p in peers {
            let mut endpoint = p.grpc_addr.clone();
            if client_tls.is_some() {
                if endpoint.starts_with("http://") {
                    endpoint = format!("https://{}", &endpoint["http://".len()..]);
                } else if !endpoint.starts_with("https://") {
                    endpoint = format!("https://{endpoint}");
                }
            } else if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
                endpoint = format!("http://{endpoint}");
            }
            let req = req.clone();
            let propose_timeout = self.propose_timeout;
            let connect_timeout = self.connect_timeout;
            let channels = channels.clone();
            let tls = client_tls.clone();
            let ep = endpoint.clone();
            tasks.push(tokio::spawn(async move {
                let cached = { channels.read().get(&ep).cloned() };
                let channel = if let Some(ch) = cached {
                    ch
                } else {
                    let mut ep_parsed = match Channel::from_shared(ep.clone()) {
                        Ok(e) => e,
                        Err(_) => return Err(()),
                    };
                    if let Some(tls_cfg) = tls {
                        ep_parsed = match ep_parsed.tls_config(tls_cfg) {
                            Ok(e) => e,
                            Err(_) => return Err(()),
                        };
                    }
                    match tokio::time::timeout(connect_timeout, ep_parsed.connect()).await {
                        Ok(Ok(ch)) => {
                            channels.write().insert(ep.clone(), ch.clone());
                            ch
                        },
                        _ => return Err(()),
                    }
                };
                let mut client = ClusterClient::new(channel);
                match tokio::time::timeout(propose_timeout, client.propose_control(req)).await {
                    Ok(Ok(resp)) => Ok(resp.into_inner().accepted),
                    _ => {
                        channels.write().remove(&ep);
                        Err(())
                    },
                }
            }));
        }

        for t in tasks {
            match t.await {
                Ok(Ok(true)) => accepts += 1,
                Ok(Ok(false)) => {},
                _ => {
                    uncertain = true;
                },
            }
        }

        // If local rejected, don't report success; but peers may have accepted (we already sent) -
        // v1: coordinator returns local decision context; uncertainty dominates.
        let out = if uncertain {
            // Can we still prove majority? only if accepts already >= majority despite uncertainty -
            // but uncertainty means some peers' state unknown; safer to report uncertain.
            LeaseQuorumOutcome {
                decision: if local_decision.accepted && accepts >= majority {
                    // majority known accepted, but had uncertainties; still conservative on "uncertain"
                    let mut d = local_decision;
                    d.accepted = false;
                    d.error_code = "CONTROL_PLANE_UNCERTAIN".to_string();
                    d.error_message = format!(
                        "uncertain peer responses; accepts={accepts} majority={majority} cluster_size={cluster_size}"
                    );
                    d
                } else {
                    ControlDecision::err(
                        "CONTROL_PLANE_UNCERTAIN",
                        format!(
                            "uncertain peer responses; accepts={accepts} majority={majority} cluster_size={cluster_size}"
                        ),
                        local_decision.current_generation,
                    )
                },
                accepts,
                cluster_size,
                uncertain: true,
            }
        } else if (local_decision.accepted && accepts >= majority) || !local_decision.accepted {
            LeaseQuorumOutcome {
                decision: local_decision,
                accepts,
                cluster_size,
                uncertain: false,
            }
        } else {
            LeaseQuorumOutcome {
                decision: ControlDecision::err(
                    "QUORUM_NOT_REACHED",
                    format!("accepts={accepts} majority={majority} cluster_size={cluster_size}"),
                    local_decision.current_generation,
                ),
                accepts,
                cluster_size,
                uncertain: false,
            }
        };
        // ADR-019：不缓存不确定结果（可能已部分提交；客户端应以同 request_id 重试并结合状态查询）。
        if !out.uncertain {
            self.cache_decision(key, request_id, &out);
        }
        self.state.record_submit_outcome(&out);
        out
    }
}

#[cfg(test)]
mod quorum_tests {
    use super::*;
    use crate::peer::service::ClusterServiceImpl;
    use sisyphus_bridge_api::cluster_proto::cluster_client::ClusterClient;
    use sisyphus_bridge_api::cluster_proto::cluster_server::ClusterServer;
    use sisyphus_bridge_api::cluster_proto::ControlProposeRequest;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tonic::transport::Server;

    async fn serve_node(
        node_id: &str,
        enabled: bool,
    ) -> (
        Arc<PeerManager>,
        Arc<ControlPlaneState>,
        String,
        tokio::task::JoinHandle<()>,
    ) {
        let mgr = Arc::new(PeerManager::new(node_id, Duration::from_secs(30)));
        let state = Arc::new(ControlPlaneState::new());
        let svc = ClusterServiceImpl::with_control_plane(mgr.clone(), None, state.clone(), enabled);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        let h = tokio::spawn(async move {
            let _ = Server::builder()
                .add_service(ClusterServer::new(svc))
                .serve_with_incoming(incoming)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        (mgr, state, addr, h)
    }

    fn lease_payload(owner: &str, ttl: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"owner": owner, "ttl_secs": ttl})).unwrap()
    }

    #[tokio::test]
    async fn n3_quorum_acquire_succeeds_with_majority() {
        let (m1, s1, a1, _h1) = serve_node("n1", true).await;
        let (m2, s2, a2, _h2) = serve_node("n2", true).await;
        let (m3, s3, a3, _h3) = serve_node("n3", true).await;
        // n1 knows n2,n3
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new("n2", a2));
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new("n3", a3));
        // others know n1 (not required for this coordinator call, but realistic)
        let _ = m2.on_peer_joined(crate::peer::manager::PeerInfo::new("n1", a1.clone()));
        let _ = m3.on_peer_joined(crate::peer::manager::PeerInfo::new("n1", a1));

        let coord = ControlPlaneCoordinator::new(m1, s1.clone(), Duration::from_secs(2));
        let out = coord
            .propose_lease(
                "r1",
                "lease:task/x",
                "lease.acquire",
                &lease_payload("n1", 30),
                0,
                1,
                3600,
            )
            .await;
        assert!(out.decision.accepted, "{out:?}");
        assert!(!out.uncertain, "{out:?}");
        assert!(out.accepts >= 2, "{out:?}");
        // at least local applied; remote peers apply via RPC handler
        assert!(s1.get("lease:task/x").is_some());
        assert!(
            s2.get("lease:task/x").is_some() || s3.get("lease:task/x").is_some(),
            "majority peers should apply"
        );
    }

    #[tokio::test]
    async fn n3_quorum_fails_when_majority_unreachable() {
        let (m1, s1, _a1, _h1) = serve_node("n1", true).await;
        // peers point to closed ports -> connect fails/uncertain
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new(
            "n2",
            "http://127.0.0.1:1",
        ));
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new(
            "n3",
            "http://127.0.0.1:1",
        ));
        let coord = ControlPlaneCoordinator::new(m1, s1, Duration::from_millis(300));
        let out = coord
            .propose_lease(
                "r1",
                "lease:task/y",
                "lease.acquire",
                &lease_payload("n1", 30),
                0,
                1,
                3600,
            )
            .await;
        assert!(!out.decision.accepted, "{out:?}");
        assert!(
            out.uncertain || out.decision.error_code == "QUORUM_NOT_REACHED",
            "{out:?}"
        );
    }

    #[tokio::test]
    async fn n3_request_id_idempotent_across_coordinator() {
        let (m1, s1, _a1, _h1) = serve_node("n1", true).await;
        let coord = ControlPlaneCoordinator::new(m1, s1, Duration::from_secs(2));
        let p = lease_payload("n1", 30);
        let a = coord.propose_lease("same-r", "lease:z", "lease.acquire", &p, 0, 1, 3600).await;
        assert!(a.decision.accepted);
        let b = coord.propose_lease("same-r", "lease:z", "lease.acquire", &p, 0, 1, 3600).await;
        assert!(b.decision.accepted);
        assert!(
            b.decision.idempotent_replay
                || b.decision.applied_generation == a.decision.applied_generation
        );
    }

    /// ADR-019 矩阵：renew/release/fencing 经协调器（快测，in-process gRPC）。
    #[tokio::test]
    async fn n3_renew_release_and_fencing_via_coordinator() {
        let (m1, s1, a1, _h1) = serve_node("n1", true).await;
        let (_m2, _s2, a2, _h2) = serve_node("n2", true).await;
        let (_m3, _s3, a3, _h3) = serve_node("n3", true).await;
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new("n2", a2));
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new("n3", a3));
        let _ = _m2.on_peer_joined(crate::peer::manager::PeerInfo::new("n1", a1));
        let coord = ControlPlaneCoordinator::new(m1, s1.clone(), Duration::from_secs(2));
        let p = lease_payload("n1", 30);
        let acq = coord.propose_lease("r1", "lease:t", "lease.acquire", &p, 0, 1, 3600).await;
        assert!(acq.decision.accepted, "{acq:?}");
        let gen = acq.decision.applied_generation;
        let renew = coord.propose_lease("r2", "lease:t", "lease.renew", &p, gen, 1, 3600).await;
        assert!(renew.decision.accepted, "{renew:?}");
        assert!(renew.decision.applied_generation > gen);
        let bad = coord.propose_lease("r3", "lease:t", "lease.renew", &p, gen, 1, 3600).await;
        assert!(!bad.decision.accepted);
        assert_eq!(bad.decision.error_code, "FENCING_MISMATCH");
        let rel = coord
            .propose_lease(
                "r4",
                "lease:t",
                "lease.release",
                &p,
                renew.decision.applied_generation,
                1,
                3600,
            )
            .await;
        assert!(rel.decision.accepted, "{rel:?}");
        assert!(s1.get("lease:t").is_none());
    }

    /// ADR-019 矩阵：分区愈合模拟（成员地址从不可达恢复）— 依赖本地 network 端口绑定与稳定时序。
    #[tokio::test]
    #[ignore = "requires dedicated runner with local network port binding and stable timing (ADR-019 partition-heal matrix)"]
    async fn n3_partition_heal_rejoin_address_update() {
        let (m1, s1, _a1, _h1) = serve_node("n1", true).await;
        let (_m2, _s2, a2, _h2) = serve_node("n2", true).await;
        let (_m3, _s3, _a3, _h3) = serve_node("n3", true).await;
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new("n2", a2.clone()));
        // n3 initially unreachable
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new(
            "n3",
            "http://127.0.0.1:1",
        ));
        let coord =
            ControlPlaneCoordinator::new(m1.clone(), s1.clone(), Duration::from_millis(500));
        let p = lease_payload("n1", 30);
        let out = coord.propose_lease("r1", "lease:heal", "lease.acquire", &p, 0, 1, 3600).await;
        // majority (n1+n2) should succeed despite n3 unreachable
        assert!(out.decision.accepted || out.uncertain, "{out:?}");
        // heal: update n3 address (PeerManager::on_peer_joined overwrites)
        let (_m3b, s3b, a3b, _h3b) = serve_node("n3-healed", true).await;
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new("n3", a3b));
        // different key to avoid lease-held; proves coordinator uses updated membership
        let out2 = coord.propose_lease("r2", "lease:heal2", "lease.acquire", &p, 0, 1, 3600).await;
        assert!(
            out2.decision.accepted,
            "{out2:?} s2={:?} s3b={:?}",
            _s2.get("lease:heal2"),
            s3b.get("lease:heal2")
        );
    }

    /// ADR-019 矩阵：回滚/门禁 — gate off 时 RPC 拒绝（DISABLED），不假成功。
    #[tokio::test]
    #[ignore = "requires dedicated runner with local network port binding (ADR-019 rollback drill)"]
    async fn rollback_gate_off_rejects_control_plane_writes() {
        let (_m, state, addr, _h) = serve_node("n1", false).await;
        let mut client = ClusterClient::connect(addr).await.unwrap();
        let resp = client
            .propose_control(ControlProposeRequest {
                request_id: "r1".into(),
                key: "lease:rb".into(),
                operation: "lease.acquire".into(),
                payload_json: lease_payload("n1", 30),
                source_node_id: "t".into(),
                expected_generation: 0,
            })
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.accepted);
        assert_eq!(resp.error_code, "DISABLED");
        assert!(state.get("lease:rb").is_none());
    }

    /// ADR-019 矩阵：少数派/分区下不可提交（双不可达）。
    #[tokio::test]
    #[ignore = "requires dedicated runner with local network port binding and stable timing (ADR-019 minority-unavailable matrix)"]
    async fn n3_minority_cannot_commit_when_quorum_unreachable() {
        let (m1, s1, _a1, _h1) = serve_node("n1", true).await;
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new(
            "n2",
            "http://127.0.0.1:1",
        ));
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new(
            "n3",
            "http://127.0.0.1:1",
        ));
        let coord = ControlPlaneCoordinator::new(m1, s1.clone(), Duration::from_millis(200));
        let out = coord
            .propose_lease(
                "r1",
                "lease:min",
                "lease.acquire",
                &lease_payload("n1", 30),
                0,
                1,
                3600,
            )
            .await;
        assert!(!out.decision.accepted);
        // local apply may have happened; but outcome is uncertain/failure (no quorum proof) - v1 conservative
        assert!(out.uncertain || out.accepts < out.cluster_size / 2 + 1);
    }

    /// ADR-019：通过 gRPC `SubmitControlLease`（协调提交）在 N=3 集群完成多数派租约。
    #[tokio::test]
    async fn n3_submit_control_lease_via_grpc_quorum() {
        let (m1, s1, _a1, _h1) = serve_node("n1", true).await;
        let (_m2, _s2, a2, _h2) = serve_node("n2", true).await;
        let (_m3, _s3, a3, _h3) = serve_node("n3", true).await;
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new("n2", a2));
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new("n3", a3));
        let coord = std::sync::Arc::new(ControlPlaneCoordinator::new(
            m1.clone(),
            s1.clone(),
            Duration::from_secs(2),
        ));
        // Start a coordinator-serving node (n1 already has basic service; build one with coordinator)
        let svc = ClusterServiceImpl::with_control_plane_coordinator(
            m1.clone(),
            None,
            s1.clone(),
            coord,
            true,
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tokio::spawn(async move {
            let _ = Server::builder()
                .add_service(ClusterServer::new(svc))
                .serve_with_incoming(incoming)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(30)).await;

        let mut client = ClusterClient::connect(addr).await.unwrap();
        let resp = client
            .submit_control_lease(ControlProposeRequest {
                request_id: "gr1".into(),
                key: "lease:grpc".into(),
                operation: "lease.acquire".into(),
                payload_json: lease_payload("n1", 30),
                source_node_id: "client".into(),
                expected_generation: 0,
            })
            .await
            .unwrap()
            .into_inner();
        assert!(resp.accepted, "{resp:?}");
        assert!(!resp.uncertain, "{resp:?}");
        assert!(resp.accepts >= 2, "{resp:?}");
        assert!(s1.get("lease:grpc").is_some());

        // rollback/gate-off via dedicated service
        let (_m4, _s4, addr4, _h4) = serve_node("n4", false).await;
        let mut c4 = ClusterClient::connect(addr4).await.unwrap();
        let r4 = c4
            .submit_control_lease(ControlProposeRequest {
                request_id: "gr2".into(),
                key: "lease:off".into(),
                operation: "lease.acquire".into(),
                payload_json: lease_payload("n1", 30),
                source_node_id: "client".into(),
                expected_generation: 0,
            })
            .await
            .unwrap()
            .into_inner();
        assert!(!r4.accepted);
        assert_eq!(r4.error_code, "DISABLED");
    }

    /// ADR-019：`GetControlLease` 只读查询（门禁/提交后状态/不确定提交指引配套）。
    #[tokio::test]
    async fn get_control_lease_read_only_and_gate() {
        // gate off
        let (_m, _s, addr, _h) = serve_node("n1", false).await;
        let mut c = ClusterClient::connect(addr).await.unwrap();
        let r = c
            .get_control_lease(sisyphus_bridge_api::cluster_proto::GetControlLeaseRequest {
                key: "lease:x".into(),
            })
            .await
            .unwrap()
            .into_inner();
        assert!(!r.found);
        assert_eq!(r.status_code, "DISABLED");

        // gate on, submit then get
        let (m1, s1, _a1, _h1) = serve_node("n1", true).await;
        let coord = std::sync::Arc::new(ControlPlaneCoordinator::new(
            m1.clone(),
            s1.clone(),
            Duration::from_secs(2),
        ));
        let svc = ClusterServiceImpl::with_control_plane_coordinator(m1, None, s1, coord, true);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tokio::spawn(async move {
            let _ = Server::builder()
                .add_service(ClusterServer::new(svc))
                .serve_with_incoming(incoming)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        let mut c = ClusterClient::connect(addr).await.unwrap();
        let nf = c
            .get_control_lease(sisyphus_bridge_api::cluster_proto::GetControlLeaseRequest {
                key: "lease:nope".into(),
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(nf.status_code, "NOT_FOUND");
        let sub = c
            .submit_control_lease(ControlProposeRequest {
                request_id: "g1".into(),
                key: "lease:get".into(),
                operation: "lease.acquire".into(),
                payload_json: lease_payload("n1", 30),
                source_node_id: "c".into(),
                expected_generation: 0,
            })
            .await
            .unwrap()
            .into_inner();
        assert!(sub.accepted, "{sub:?}");
        let got = c
            .get_control_lease(sisyphus_bridge_api::cluster_proto::GetControlLeaseRequest {
                key: "lease:get".into(),
            })
            .await
            .unwrap()
            .into_inner();
        assert!(got.found);
        assert_eq!(got.status_code, "OK");
        assert_eq!(got.owner, "n1");
        assert!(got.generation >= 1);
    }

    /// ADR-019：成员变化/leader 字典序变化模拟下，协调仍按当前成员视图与多数派/不确定=错误判定。
    #[tokio::test]
    async fn coordinator_follows_membership_changes_fail_closed() {
        let (m1, s1, _a1, _h1) = serve_node("n1", true).await;
        // single node cluster (leader is n1) can commit locally
        let coord = ControlPlaneCoordinator::new(m1.clone(), s1, Duration::from_millis(200));
        let ok = coord
            .propose_lease(
                "r1",
                "lease:solo",
                "lease.acquire",
                &lease_payload("n1", 30),
                0,
                1,
                3600,
            )
            .await;
        assert!(ok.decision.accepted, "{ok:?}");

        // add two unreachable peers => majority uncertain/fail-closed, no false success
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new(
            "n0",
            "http://127.0.0.1:1",
        )); // n0 would become leader by lexicographic order
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new(
            "n2",
            "http://127.0.0.1:1",
        ));
        let out = coord
            .propose_lease(
                "r2",
                "lease:churn",
                "lease.acquire",
                &lease_payload("n1", 30),
                0,
                1,
                3600,
            )
            .await;
        assert!(!out.decision.accepted, "{out:?}");
        assert!(
            out.uncertain
                || out.decision.error_code == "QUORUM_NOT_REACHED"
                || out.decision.error_code == "CONTROL_PLANE_UNCERTAIN",
            "{out:?}"
        );

        // peer leaves => membership view changes; still fail-closed while peers unreachable
        let _ = m1.on_peer_left("n2", "test");
        let out2 = coord
            .propose_lease(
                "r3",
                "lease:churn2",
                "lease.acquire",
                &lease_payload("n1", 30),
                0,
                1,
                3600,
            )
            .await;
        assert!(!out2.decision.accepted, "{out2:?}");
    }

    /// ADR-019：协调节点本地拒绝的请求不得广播到 peer（不得修改 peer 状态）。
    #[tokio::test]
    async fn rejected_local_proposal_is_not_broadcast() {
        let (m1, s1, _a1, _h1) = serve_node("n1", true).await;
        let (_m2, s2, a2, _h2) = serve_node("n2", true).await;
        let _ = m1.on_peer_joined(crate::peer::manager::PeerInfo::new("n2", a2.clone()));
        // n1 本地已持有该租约；来自 n2 的同 key acquire 必须本地拒绝。
        assert!(
            s1.apply(
                "seed",
                "lease:held",
                "lease.acquire",
                &lease_payload("n1", 30),
                0,
                1,
                300,
            )
            .accepted
        );
        let coord = Arc::new(ControlPlaneCoordinator::new(m1, s1, Duration::from_secs(2)));
        let out = coord
            .propose_lease(
                "r-reject",
                "lease:held",
                "lease.acquire",
                &lease_payload("n2", 30),
                0,
                1,
                300,
            )
            .await;
        assert!(!out.decision.accepted, "{out:?}");
        assert_eq!(out.decision.error_code, "LEASE_HELD");
        assert_eq!(out.accepts, 0);
        assert!(!out.uncertain);
        assert!(
            s2.get("lease:held").is_none(),
            "被拒绝的提案不得修改 peer 状态"
        );
    }

    /// ADR-019：不确定提交指引流程（同 request_id 重试 + GetControlLease 核对）。
    #[tokio::test]
    async fn uncertain_submission_guidance_retry_and_get() {
        let (m1, s1, _a1, _h1) = serve_node("n1", true).await;
        // local-only cluster for deterministic query; simulate "client retries with same request_id"
        let coord = Arc::new(ControlPlaneCoordinator::new(
            m1.clone(),
            s1.clone(),
            Duration::from_secs(2),
        ));
        let svc = ClusterServiceImpl::with_control_plane_coordinator(m1, None, s1, coord, true);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tokio::spawn(async move {
            let _ = Server::builder()
                .add_service(ClusterServer::new(svc))
                .serve_with_incoming(incoming)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        let mut c = ClusterClient::connect(addr).await.unwrap();
        let req = ControlProposeRequest {
            request_id: "same-u".into(),
            key: "lease:u".into(),
            operation: "lease.acquire".into(),
            payload_json: lease_payload("n1", 30),
            source_node_id: "client".into(),
            expected_generation: 0,
        };
        let a = c.submit_control_lease(req.clone()).await.unwrap().into_inner();
        assert!(a.accepted, "{a:?}");
        let got = c
            .get_control_lease(sisyphus_bridge_api::cluster_proto::GetControlLeaseRequest {
                key: "lease:u".into(),
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(got.status_code, "OK");
        assert_eq!(got.last_request_id, "same-u");
        // retry with same request_id (idempotent)
        let b = c.submit_control_lease(req).await.unwrap().into_inner();
        assert!(b.accepted, "{b:?}");
        assert_eq!(b.applied_generation, a.applied_generation);
    }
}
