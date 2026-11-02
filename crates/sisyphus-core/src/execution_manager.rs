use crate::{CoreError, ExecutionStatus, Executor};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sisyphus_mem::{MfsStore, SopSqlStore};
use sisyphus_types::{
    CogMap, ExecutionContext, ExecutionEvent, ExecutorRouter, SOPGraph, SOPGraphConfig,
    ToolExecutor,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// A single recorded cognitive decision from the loop (explainability primitive).
///
/// Carries the SOP it belonged to plus a wall-clock timestamp so viewers can
/// order decisions across executions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CognitiveTraceEntry {
    /// Milliseconds since epoch when the event was recorded.
    pub timestamp_ms: u128,
    /// SOP id the event belongs to.
    pub sop_id: String,
    /// The decision event itself (CognitiveStep / MicroReflection / ...).
    pub event: ExecutionEvent,
}

/// Thread-safe ring buffer of recent cognitive decisions.
///
/// Used to make the loop's decision stream queryable after the fact, which is
/// the data backbone for the explainability (cognitive trace) UI. Oldest
/// entries are dropped once `capacity` is reached.
pub struct CognitiveTraceBuffer {
    entries: Mutex<VecDeque<CognitiveTraceEntry>>,
    capacity: usize,
}

impl CognitiveTraceBuffer {
    /// Create a buffer that keeps at most `capacity` entries.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(capacity.min(1024))),
            capacity: capacity.max(1),
        }
    }

    /// Record an event, evicting the oldest entry when over capacity.
    pub fn push(&self, entry: CognitiveTraceEntry) {
        let mut entries = self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// Return the most recent `limit` entries (newest first).
    pub fn recent(&self, limit: usize) -> Vec<CognitiveTraceEntry> {
        let entries = self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let limit = limit.min(entries.len());
        entries.iter().rev().take(limit).cloned().collect()
    }

    /// Number of entries currently held.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_empty()
    }
}

/// ExecutionManager 负责 SOP 图谱执行的底层编排，
/// 包括工具路由、状态持久化与实时状态跟踪。
pub struct ExecutionManager {
    pub executor: Executor,
    pub state_store: Arc<MfsStore>,
    pub sop_store: Arc<SopSqlStore>,
    pub tool_router: Arc<ExecutorRouter>,
    pub active_executions: Arc<DashMap<String, ExecutionStatus>>,
    pub cognitive_maps: Arc<DashMap<String, CogMap>>,
    pub event_subscribers: Arc<DashMap<String, Vec<mpsc::UnboundedSender<ExecutionEvent>>>>,
    /// Ring buffer of recent cognitive decisions, queryable for explainability.
    pub cognitive_trace: Arc<CognitiveTraceBuffer>,
    /// 可选的异步持久化通道。由 writer task 写入 SQL 存储，
    /// 使认知决策跨会话可检索（历史）。
    trace_persist_tx: Option<mpsc::UnboundedSender<TracePersistCmd>>,
}

/// writer task 的指令：落盘一条记录，或作为 flush 屏障确认已排空。
enum TracePersistCmd {
    /// 落盘一条认知决策。`ExecutionEvent` 远大于 oneshot sender，故装箱。
    Record(Box<(String, CognitiveTraceEntry)>),
    /// flush 屏障：writer 处理到此处即说明此前所有指令都已落盘。
    ///
    /// 回执携带**自上次 flush 以来**累积的写入失败信息（条数 + 首条错误），
    /// 使调用方能区分「已落盘」与「已排队但写失败了」。后者此前仅由
    /// `tracing::warn` 记录，导致 flush 对彻底失败的写入谎报成功。
    Flush(tokio::sync::oneshot::Sender<TraceFlushAck>),
}

/// flush 屏障回执的等待上限。
///
/// 转发自 [`sisyphus_mem::TRACE_FLUSH_BARRIER_TIMEOUT`]，与 SQLite 的
/// `busy_timeout` 同源。**不要**在此另写一个字面量：历史上这里写的 5s
/// 与 sqlx 的 `busy_timeout` 默认值恰好相等，导致两个独立的等待上限撞在
/// 一起——写锁被占满时单条写入就能耗尽整个 busy_timeout，屏障随即超时，
/// 对已成功落盘的写入谎报 “durability not guaranteed”。
///
/// 屏障为 FIFO，轮到它时前面的指令均已落盘，因此它要等的只是 writer
/// 手头那一条写入。两者同源后，“屏障 > 单条写入最坏耗时”由构造式保证，
/// 不再依赖两个无关数字恰好不相等。
const TRACE_FLUSH_BARRIER_TIMEOUT: std::time::Duration = sisyphus_mem::TRACE_FLUSH_BARRIER_TIMEOUT;

/// flush 屏障的回执：自上次 flush 起写入失败的累积情况。
#[derive(Debug, Default)]
struct TraceFlushAck {
    /// 落盘失败的记录条数。
    failed: usize,
    /// 第一条失败的原始错误。后续错误只计数，避免回执随失败数无限膨胀。
    first_error: Option<String>,
}

impl ExecutionManager {
    pub fn new(
        state_store: Arc<MfsStore>,
        sop_store: Arc<SopSqlStore>,
        tool_executor: Arc<dyn ToolExecutor>,
        test_executor: Arc<dyn sisyphus_types::TestExecutor>,
        intervention_manager: Arc<crate::intervention_manager::InterventionManager>,
        internalization_manager: Arc<crate::internalization_manager::InternalizationManager>,
        visual_manager: Option<Arc<sisyphus_visual::VisualPerceptionManager>>,
    ) -> Self {
        let mut em = Self {
            executor: Executor::new(
                test_executor,
                intervention_manager,
                internalization_manager,
                visual_manager,
            ),
            state_store,
            sop_store,
            tool_router: Arc::new(ExecutorRouter::new(tool_executor)),
            active_executions: Arc::new(DashMap::new()),
            cognitive_maps: Arc::new(DashMap::new()),
            event_subscribers: Arc::new(DashMap::new()),
            cognitive_trace: Arc::new(CognitiveTraceBuffer::new(200)),
            trace_persist_tx: None,
        };
        em.init_trace_persistence();
        em
    }

    /// 获取内部执行器实例（用于配置热重载）。
    pub fn get_executor(&self) -> Option<&Executor> {
        Some(&self.executor)
    }

    /// 运行时应用执行器配置（热重载支持）。
    ///
    /// 委托给 [`Executor::apply_config`]：就地增删信号量许可，因此在途执行
    /// 也会立刻看到新的并发上限。旧实现通过替换 `Arc<Semaphore>` 实现，
    /// 只能影响之后新建的批次，且要求 `&mut self`——而内核持有的是
    /// `Arc<ExecutionManager>`，实际上永远调不到。
    pub fn apply_executor_config(&self, executor_config: &sisyphus_types::ExecutorConfig) {
        self.executor.apply_config(executor_config);
    }

    /// 启动认知决策异步持久化 writer：每个记录事件写入 SQL `cognitive_traces` 表。
    ///
    /// 在 `new()` 中自动调用，随 `state_store` 就地落盘，无需调用方额外配置。
    fn init_trace_persistence(&mut self) {
        let store = Arc::clone(&self.state_store);
        let (tx, mut rx) = mpsc::unbounded_channel::<TracePersistCmd>();
        tokio::spawn(async move {
            // 自上次 flush 起累积的写入失败，由下一个屏障回传给调用方。
            let mut ack = TraceFlushAck::default();
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    TracePersistCmd::Record(payload) => {
                        let (sop_id, entry) = *payload;
                        if let Err(e) = store
                            .record_cognitive_trace(
                                "default",
                                &sop_id,
                                entry.timestamp_ms,
                                &entry.event,
                            )
                            .await
                        {
                            ack.failed += 1;
                            if ack.first_error.is_none() {
                                ack.first_error = Some(e.to_string());
                            }
                            tracing::warn!(
                                error = %e,
                                failed_since_flush = ack.failed,
                                "Failed to persist cognitive trace"
                            );
                        }
                    },
                    // 队列是 FIFO 且 writer 串行处理：轮到屏障时前面的指令必然已落盘。
                    // 回执带上期间失败的条数，让调用方不会把「写失败」当成「已落盘」。
                    TracePersistCmd::Flush(reply) => {
                        let _ = reply.send(std::mem::take(&mut ack));
                    },
                }
            }
        });
        self.trace_persist_tx = Some(tx);
    }

    /// 等待此前所有认知决策记录落盘完成，并报告期间的写入失败。
    ///
    /// [`record_cognitive_trace`] 只把指令投入 channel 就返回，因此进程退出、
    /// manager 被 drop 或测试重建实例时，尚在队列里的记录会静默丢失。
    /// 本方法往队列里追加一个屏障指令并等其回执，从而给出真正的
    /// “已持久化”保证：返回 `Ok(())` 时，此前记录均已写入 `cognitive_traces`。
    ///
    /// **失败不再静默**：若屏障之前有记录写入失败（磁盘只读、SQL 锁、
    /// 库损坏等），返回 [`CoreError::MfsSaveError`] 并带上失败条数与首条错误。
    /// 修复前这些失败仅由 `tracing::warn` 记录而 `flush` 返回 `()`，
    /// 使调用方（bridge 优雅关闭）把彻底失败的写入当成已持久化。
    ///
    /// 未启用持久化（`trace_persist_tx` 为 `None`）或 writer 已退出时返回
    /// [`CoreError::MfsSaveError`]：此时无法保证落盘，调用方必须知情。
    pub async fn flush_cognitive_traces(&self) -> Result<(), CoreError> {
        let Some(tx) = &self.trace_persist_tx else {
            return Err(CoreError::MfsSaveError(
                "cognitive trace persistence is disabled; flush cannot guarantee durability"
                    .to_string(),
            ));
        };
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<TraceFlushAck>();
        if tx.send(TracePersistCmd::Flush(ack_tx)).is_err() {
            // writer 已退出：无法保证落盘，也不阻塞调用方。
            return Err(CoreError::MfsSaveError(
                "cognitive trace writer has exited; queued traces were not persisted".to_string(),
            ));
        }
        // writer 异常退出时回执被丢弃；超时避免永久挂起调用方。
        // 超时同样是「无法保证落盘」，必须显式报告而非静默放行。
        match tokio::time::timeout(TRACE_FLUSH_BARRIER_TIMEOUT, ack_rx).await {
            Ok(Ok(ack)) if ack.failed > 0 => Err(CoreError::MfsSaveError(format!(
                "{} cognitive trace(s) failed to persist since last flush; first error: {}",
                ack.failed,
                ack.first_error.as_deref().unwrap_or("unknown")
            ))),
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) => Err(CoreError::MfsSaveError(
                "cognitive trace writer dropped the flush barrier before acknowledging".to_string(),
            )),
            Err(_) => Err(CoreError::MfsSaveError(format!(
                "cognitive trace flush timed out after {}s; durability not guaranteed",
                TRACE_FLUSH_BARRIER_TIMEOUT.as_secs()
            ))),
        }
    }

    /// 通知管理器有新工具部署，以便使缓存失效。
    pub fn notify_tool_deployed(&self, tool_id: &str) {
        tracing::info!(tool_id = %tool_id, "Tool deployed; signaling executor cache invalidation");

        if let Some(invalidator) = self.tool_router.default.as_cache_invalidatable() {
            invalidator.invalidate_cache(tool_id);
        }
    }

    /// 将 SOP 图谱持久化到状态存储。
    pub async fn save_sop(
        &self,
        sop_id: &str,
        project_id: &str,
        graph: &SOPGraph,
    ) -> Result<(), CoreError> {
        let config =
            SOPGraphConfig::from_graph(graph).map_err(|e| CoreError::Internal(e.to_string()))?;

        self.sop_store
            .save_sop(
                sop_id,
                project_id,
                "Sisyphus SOP",
                Some("1.0.0".into()),
                &config,
            )
            .await
            .map_err(|e| CoreError::MfsSaveError(e.to_string()))
    }

    /// 从状态存储加载 SOP 图谱。
    /// 返回所有已注册 SOP 的 ID 列表。
    pub async fn list_sops(&self, project_id: Option<&str>) -> Result<Vec<String>, CoreError> {
        self.sop_store.list_sops(project_id).await.map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// 从状态存储加载 SOP 图谱。
    pub async fn load_sop(
        &self,
        sop_id: &str,
        project_id: &str,
    ) -> Result<Arc<SOPGraph>, CoreError> {
        let config = self
            .sop_store
            .load_sop(sop_id, project_id)
            .await
            .map_err(|e| CoreError::MfsLoadError(e.to_string()))?
            .ok_or_else(|| {
                CoreError::SopNotFound(format!("SOP {sop_id} not found in project {project_id}"))
            })?;

        Ok(SOPGraph::from_config(
            &config,
            self.tool_router.clone(),
            self.executor.test_executor.clone(),
        )?)
    }

    /// 将执行事件广播给某个 SOP 的所有订阅者。
    ///
    /// 认知决策事件 (CognitiveStep / MicroReflection) 同时写入
    /// [`CognitiveTraceBuffer`]，使决策流在事后可查询（可解释性）。
    pub fn broadcast_event(&self, sop_id: &str, event: ExecutionEvent) {
        if let Some(subscribers) = self.event_subscribers.get(sop_id) {
            let mut subs = subscribers.clone();
            subs.retain(|tx| tx.send(event.clone()).is_ok());
            self.event_subscribers.insert(sop_id.to_string(), subs);
        }
        self.record_cognitive_trace(sop_id, event);
    }

    /// 将认知决策事件记录到环形缓冲（如果该事件类型值得留存），并异步落盘。
    pub fn record_cognitive_trace(&self, sop_id: &str, event: ExecutionEvent) {
        let keep = matches!(
            event,
            ExecutionEvent::CognitiveStep { .. } | ExecutionEvent::MicroReflection { .. }
        );
        if keep {
            let timestamp_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let entry = CognitiveTraceEntry {
                timestamp_ms,
                sop_id: sop_id.to_string(),
                event,
            };
            self.cognitive_trace.push(entry.clone());
            if let Some(tx) = &self.trace_persist_tx {
                // 持久化是尽力而为：channel 关闭（writer 已退出）时静默丢弃。
                let _ = tx.send(TracePersistCmd::Record(Box::new((
                    sop_id.to_string(),
                    entry,
                ))));
            }
        }
    }

    /// 返回最近的 `limit` 条认知决策（新在前）。
    pub fn recent_cognitive_trace(&self, limit: usize) -> Vec<CognitiveTraceEntry> {
        self.cognitive_trace.recent(limit)
    }

    /// 从 SQL 存储检索认知决策历史（跨会话），新在前。
    ///
    /// `sop_id` 为 `None` 时返回全部 SOP 的记录。
    pub async fn cognitive_trace_history(
        &self,
        project_id: &str,
        sop_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CognitiveTraceEntry>, CoreError> {
        let records = self
            .state_store
            .get_cognitive_traces(project_id, sop_id, limit)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        Ok(records
            .into_iter()
            .map(|r| CognitiveTraceEntry {
                timestamp_ms: r.timestamp_ms,
                sop_id: r.sop_id,
                event: r.event,
            })
            .collect())
    }

    /// 聚合最近的认知决策历史为跨会话健康摘要。
    ///
    /// `sop_id` 为 `None` 时聚合全部 SOP；`limit` 限制参与聚合的记录数。
    pub async fn cognitive_trace_summary(
        &self,
        project_id: &str,
        sop_id: Option<&str>,
        limit: usize,
    ) -> Result<sisyphus_types::CognitiveTraceSummary, CoreError> {
        self.state_store
            .cognitive_trace_summary(project_id, sop_id, limit)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// 在给定上下文中执行图谱的底层原语。
    pub async fn execute(
        &self,
        sop_graph: Arc<SOPGraph>,
        start_node: petgraph::graph::NodeIndex,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        supervisor: Option<Arc<dyn sisyphus_types::RuntimeSupervisor>>,
    ) -> Result<Arc<ExecutionContext>, CoreError> {
        self.executor
            .execute_with_context(
                sop_graph,
                start_node,
                initial_ctx,
                event_tx,
                self.tool_router.clone(),
                supervisor,
                None,
                None,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::CognitiveMetrics;
    use std::collections::HashMap;

    fn step_event(step: &str) -> ExecutionEvent {
        ExecutionEvent::CognitiveStep {
            step: step.to_string(),
            message: format!("{step} message"),
            metrics: Some(CognitiveMetrics {
                iteration: 1,
                deviation: 0.1,
                confidence: 0.9,
                progress: 50.0,
                token_cost: 10,
                stability: 0.8,
                info_gain: 0.2,
            }),
        }
    }

    fn micro_reflection() -> ExecutionEvent {
        ExecutionEvent::MicroReflection {
            node_name: "n1".into(),
            prediction: "expected".into(),
            actual: "actual".into(),
            decision: "ADJUST".into(),
            updates: Some(HashMap::new()),
            reason: Some("deviation too high".into()),
        }
    }

    #[test]
    fn test_trace_buffer_push_and_recent() {
        let buf = CognitiveTraceBuffer::new(10);
        buf.push(CognitiveTraceEntry {
            timestamp_ms: 1,
            sop_id: "s1".into(),
            event: step_event("Predicting"),
        });
        buf.push(CognitiveTraceEntry {
            timestamp_ms: 2,
            sop_id: "s1".into(),
            event: micro_reflection(),
        });

        assert_eq!(buf.len(), 2);
        let recent = buf.recent(10);
        // Newest first.
        assert_eq!(recent.len(), 2);
        assert!(matches!(
            recent[0].event,
            ExecutionEvent::MicroReflection { .. }
        ));
        assert!(matches!(
            recent[1].event,
            ExecutionEvent::CognitiveStep { .. }
        ));
        assert_eq!(recent[0].sop_id, "s1");
    }

    #[test]
    fn test_trace_buffer_evicts_oldest() {
        let buf = CognitiveTraceBuffer::new(3);
        for i in 0..5 {
            buf.push(CognitiveTraceEntry {
                timestamp_ms: i as u128,
                sop_id: "s1".into(),
                event: step_event(&format!("step{i}")),
            });
        }
        assert_eq!(buf.len(), 3, "capacity must cap the buffer");
        let recent = buf.recent(10);
        assert_eq!(recent.len(), 3);
        // Oldest (0,1) evicted; newest kept, newest first.
        assert_eq!(recent[0].timestamp_ms, 4);
        assert_eq!(recent[2].timestamp_ms, 2);
    }

    #[test]
    fn test_trace_buffer_recent_limit() {
        let buf = CognitiveTraceBuffer::new(10);
        for i in 0..5 {
            buf.push(CognitiveTraceEntry {
                timestamp_ms: i as u128,
                sop_id: "s1".into(),
                event: step_event("step"),
            });
        }
        let two = buf.recent(2);
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].timestamp_ms, 4);
        assert_eq!(two[1].timestamp_ms, 3);
        assert!(buf.recent(0).is_empty());
        assert!(!buf.is_empty());
    }
}
