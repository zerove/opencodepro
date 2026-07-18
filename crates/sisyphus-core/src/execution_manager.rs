use crate::{CoreError, ExecutionStatus, Executor};
use dashmap::DashMap;
use sisyphus_mem::{MfsStore, SopSqlStore};
use sisyphus_types::{
    CogMap, ExecutionContext, ExecutionEvent, ExecutorRouter, SOPGraph, SOPGraphConfig,
    ToolExecutor,
};
use std::sync::Arc;
use tokio::sync::mpsc;

/// ExecutionManager handles the low-level orchestration of SOP graph execution,
/// including tool routing, state persistence, and real-time status tracking.
pub struct ExecutionManager {
    pub executor: Executor,
    pub state_store: Arc<MfsStore>,
    pub sop_store: Arc<SopSqlStore>,
    pub tool_router: Arc<ExecutorRouter>,
    pub active_executions: Arc<DashMap<String, ExecutionStatus>>,
    pub cognitive_maps: Arc<DashMap<String, CogMap>>,
    pub event_subscribers: Arc<DashMap<String, Vec<mpsc::UnboundedSender<ExecutionEvent>>>>,
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
        Self {
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
        }
    }

    /// Notifies the manager that a new tool has been deployed, allowing for cache invalidation.
    pub fn notify_tool_deployed(&self, tool_id: &str) {
        tracing::info!(tool_id = %tool_id, "Tool deployed; signaling executor cache invalidation");

        if let Some(invalidator) = self.tool_router.default.as_cache_invalidatable() {
            invalidator.invalidate_cache(tool_id);
        }
    }

    /// Persist a SOP graph to the state store.
    pub async fn save_sop(
        &self,
        sop_id: &str,
        project_id: &str,
        graph: &SOPGraph,
    ) -> Result<(), CoreError> {
        let config = SOPGraphConfig::from_graph(graph);

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

    /// Load a SOP graph from the state store.
    /// Returns a list of all registered SOP IDs.
    pub async fn list_sops(&self, project_id: Option<&str>) -> Result<Vec<String>, CoreError> {
        self.sop_store
            .list_sops(project_id)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// Load a SOP graph from the state store.
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

    /// Broadcast an execution event to all subscribers of a specific SOP.
    pub fn broadcast_event(&self, sop_id: &str, event: ExecutionEvent) {
        if let Some(subscribers) = self.event_subscribers.get(sop_id) {
            let mut subs = subscribers.clone();
            subs.retain(|tx| tx.send(event.clone()).is_ok());
            self.event_subscribers.insert(sop_id.to_string(), subs);
        }
    }

    /// Low-level primitive to execute a graph within a given context.
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
