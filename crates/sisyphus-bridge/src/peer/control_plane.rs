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
    /// request_id -> (generation, result snapshot key fields) for idempotent replay
    responses: HashMap<String, (u64, bool, String, String)>,
}

pub struct ControlPlaneState {
    leases: RwLock<HashMap<String, LeaseEntry>>,
    now_unix_ms: Box<dyn Fn() -> u64 + Send + Sync>,
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
        }
    }

    #[cfg(test)]
    pub fn with_fixed_now(now_ms: u64) -> Self {
        Self {
            leases: RwLock::new(HashMap::new()),
            now_unix_ms: Box::new(move || now_ms),
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
        let mut g = self.leases.write();
        if let Some(e) = g.get(key) {
            if Self::is_expired(e, now) {
                g.remove(key);
                return None;
            }
            return Some(e.record.clone());
        }
        None
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

        // Expire stale
        if g.get(key).is_some_and(|e| Self::is_expired(e, now)) {
            g.remove(key);
        }

        // Idempotent replay (per key)
        if let Some(e) = g.get(key) {
            if let Some((gen, accepted, code, msg)) = e.responses.get(request_id).cloned() {
                let mut d = if accepted {
                    ControlDecision::ok(gen, e.record.clone(), true)
                } else {
                    let mut d = ControlDecision::err(&code, msg, e.record.generation);
                    d.idempotent_replay = true;
                    d
                };
                d.idempotent_replay = true;
                return d;
            }
        } else {
            // No entry: check a global-ish replay? For v1 keep per-key; acquire on missing key has no prior entry.
            // We'll store replay only after we have an entry context where relevant.
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
                if let Some(e) = g.get(key) {
                    if !Self::is_expired(e, now) {
                        let cur = e.record.generation;
                        let d = ControlDecision::err(
                            "LEASE_HELD",
                            format!("lease held by {}", e.record.owner),
                            cur,
                        );
                        // record replay on existing entry
                        let e = g.get_mut(key).expect("exists");
                        e.responses.insert(
                            request_id.to_string(),
                            (cur, false, d.error_code.clone(), d.error_message.clone()),
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
                let mut responses = HashMap::new();
                responses.insert(
                    request_id.to_string(),
                    (generation, true, String::new(), String::new()),
                );
                g.insert(
                    key.to_string(),
                    LeaseEntry {
                        record: record.clone(),
                        expires_at: now + Duration::from_secs(ttl),
                        responses,
                    },
                );
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
                    let gen = e.record.generation;
                    g.remove(key);
                    return ControlDecision::err("LEASE_NOT_FOUND", "lease expired", gen);
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
                        (cur, false, d.error_code.clone(), d.error_message.clone()),
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
                        (cur, false, d.error_code.clone(), d.error_message.clone()),
                    );
                    return d;
                }
                if operation == "lease.release" {
                    let cur = e.record.generation;
                    let rec = e.record.clone();
                    e.responses.insert(
                        request_id.to_string(),
                        (cur, true, String::new(), String::new()),
                    );
                    g.remove(key);
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
                        (cur, false, d.error_code.clone(), d.error_message.clone()),
                    );
                    return d;
                }
                e.record.generation += 1;
                e.record.expires_at_unix_ms = now_ms.saturating_add(ttl.saturating_mul(1000));
                e.record.last_request_id = request_id.to_string();
                e.expires_at = now + Duration::from_secs(ttl);
                let gen = e.record.generation;
                let rec = e.record.clone();
                e.responses.insert(
                    request_id.to_string(),
                    (gen, true, String::new(), String::new()),
                );
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
}
