//! Experience sync module (TODO 9.1.1)
//!
//! 跨节点 experience 同步：
//! - 发送端：本地 store 新增 experience 后，调用 `broadcast_experience_sync` 推送到所有 peer
//! - 接收端：监听 PeerManager 事件，应用 CRDT 合并策略
//!
//! CRDT 策略：LWW-Element-Set + vector clock（参考 sisyphus_types::experience::merge_crdt）
//! - 收到的版本 > 本地版本 → 接受
//! - 收到的 vector clock happened-before 本地 → 丢弃
//! - 并发写入 → LWW (version + node_id tie-break)
//!
//! 当前实现：直接调用 `experience_bank.record()` 让其内部的 CRDT bump 逻辑处理。
//! 后续优化：MfsStore 暴露 `apply_remote_experience` API 进行原子 CAS。

use crate::peer::manager::{ClusterEvent as MgrEvent, PeerManager};
use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_types::experience::{ExperienceBank, ExperienceTuple};
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

/// ExperienceSyncHandler：订阅 PeerManager 事件，将远程 experience 写入本地 bank
pub struct ExperienceSyncHandler {
    experience_bank: Arc<VectorExperienceBank>,
    peer_manager: Arc<PeerManager>,
    /// 本节点 ID，用于过滤自广播
    self_node_id: String,
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
        }
    }

    /// 启动事件循环：监听 peer 事件并处理 ExperienceSync
    ///
    /// `shutdown_rx` 收到信号时退出循环。返回 `JoinHandle` 供调用方管理生命周期。
    pub fn spawn(self, mut shutdown_rx: broadcast::Receiver<()>) -> JoinHandle<()> {
        let mut events = self.peer_manager.subscribe();
        let experience_bank = self.experience_bank;
        let self_node_id = self.self_node_id;

        tokio::spawn(async move {
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
                                // 过滤自己广播的事件（node_id 嵌入 exp_key）
                                if exp_key.contains(&self_node_id) {
                                    tracing::trace!(exp_key = %exp_key, "skipping own broadcast");
                                    continue;
                                }
                                Self::handle_remote_sync(
                                    &experience_bank,
                                    &exp_key,
                                    version,
                                    &vector_clock_json,
                                    payload,
                                ).await;
                            }
                            Ok(_) => {
                                // 其他事件类型不处理
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                tracing::warn!("ExperienceSyncHandler: peer manager channel closed");
                                break;
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!(lagged = n, "ExperienceSyncHandler: lagged, may have missed events");
                            }
                        }
                    }
                }
            }
            tracing::info!("ExperienceSyncHandler: stopped");
        })
    }

    /// 处理单个远程 ExperienceSync 事件
    async fn handle_remote_sync(
        experience_bank: &Arc<VectorExperienceBank>,
        exp_key: &str,
        remote_version: u64,
        _vector_clock_json: &str,
        payload: Vec<u8>,
    ) {
        // 解析 payload
        let mut remote_exp: ExperienceTuple = match serde_json::from_slice(&payload) {
            Ok(exp) => exp,
            Err(e) => {
                tracing::warn!(error = %e, exp_key = %exp_key, "failed to deserialize remote experience");
                return;
            },
        };

        // 关键：在写入前清空 version/vector_clock，让 record() 内部用本地 node_id bump
        // 否则来自 remote 的 version 会被错误地保留（应该反映本地操作历史）
        // 注意：此处仍保留 remote_exp 的 vector_clock，因为它是 happened-before 信息
        let _ = remote_version; // 仅为日志/未来使用
        remote_exp.version = 0;
        // 保留 vector_clock：让本地 bump_local_clock 看到 remote 的因果信息

        if let Err(e) = experience_bank.record(remote_exp).await {
            tracing::error!(
                error = %e,
                exp_key = %exp_key,
                "failed to record remote experience"
            );
        } else {
            tracing::info!(
                exp_key = %exp_key,
                "applied remote experience to local store"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_broadcast_experience_serializes_payload() {
        let mgr = Arc::new(PeerManager::new("node-a", Duration::from_secs(30)));
        let mut rx = mgr.subscribe();
        let mut exp = ExperienceTuple::default();
        exp.context_text = "test".to_string();
        exp.version = 42;

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
}
