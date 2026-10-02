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
    experience_origin_from_key, ExperienceBank, ExperienceQuery, ExperienceTuple,
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
                // 过滤自己广播的事件：主键里编码了来源节点，解析出来与本节点
                // 相同即丢弃。不能用后缀包含判断——origin 不是主键的末段，
                // 那会让 "node-a" 连带吞掉 "node-ab" 的经验。
                if experience_origin_from_key(&exp_key) == Some(self_node_id.as_str()) {
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
        vector_clock_json: &str,
        payload: Vec<u8>,
    ) {
        // 解析 payload
        let remote_exp: ExperienceTuple = match serde_json::from_slice(&payload) {
            Ok(exp) => exp,
            Err(e) => {
                tracing::warn!(error = %e, exp_key = %exp_key, "failed to deserialize remote experience");
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
            },
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
