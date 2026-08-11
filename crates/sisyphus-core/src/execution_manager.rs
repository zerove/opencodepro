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
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// Return the most recent `limit` entries (newest first).
    pub fn recent(&self, limit: usize) -> Vec<CognitiveTraceEntry> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let limit = limit.min(entries.len());
        entries.iter().rev().take(limit).cloned().collect()
    }

    /// Number of entries currently held.
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
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
    /// 可选的异步持久化通道：(sop_id, entry)。由 writer task 写入 SQL 存储，
    /// 使认知决策跨会话可检索（历史）。
    trace_persist_tx: Option<mpsc::UnboundedSender<(String, CognitiveTraceEntry)>>,
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

    /// 启动认知决策异步持久化 writer：每个记录事件写入 SQL `cognitive_traces` 表。
    ///
    /// 在 `new()` 中自动调用，随 `state_store` 就地落盘，无需调用方额外配置。
    fn init_trace_persistence(&mut self) {
        let store = Arc::clone(&self.state_store);
        let (tx, mut rx) = mpsc::unbounded_channel::<(String, CognitiveTraceEntry)>();
        tokio::spawn(async move {
            while let Some((sop_id, entry)) = rx.recv().await {
                if let Err(e) = store
                    .record_cognitive_trace("default", &sop_id, entry.timestamp_ms, &entry.event)
                    .await
                {
                    tracing::warn!(error = %e, "Failed to persist cognitive trace");
                }
            }
        });
        self.trace_persist_tx = Some(tx);
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
        self.sop_store
            .list_sops(project_id)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))
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
                let _ = tx.send((sop_id.to_string(), entry));
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
