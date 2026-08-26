//! DAG 执行器 — 拓扑感知的节点调度与事件流。

use futures::future::BoxFuture;
use petgraph::algo::toposort;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::{CoreError, ExecutionContext, ExecutionEvent, NodeOutput, SOPGraph};
use sisyphus_types::{RuntimeSupervisor, SupervisorDecision};

/// 基于 DAG 的执行器，以并行节点调度运行 SOP 图谱，
/// 并支持可选的事件流。
pub struct Executor {
    pub concurrency_limit: Arc<tokio::sync::Semaphore>,
    pub graph_cache: Arc<dashmap::DashMap<String, Arc<SOPGraph>>>,
    pub prediction_cache: Arc<dashmap::DashMap<String, String>>,
    pub output_cache: Arc<crate::performance::OutputCache>,
    pub test_executor: Arc<dyn sisyphus_types::TestExecutor>,
    pub visual_manager: Option<Arc<sisyphus_visual::VisualPerceptionManager>>,
    pub intervention_manager: Arc<crate::intervention_manager::InterventionManager>,
    pub internalization_manager: Arc<crate::internalization_manager::InternalizationManager>,
}

impl Executor {
    pub fn new(
        test_executor: Arc<dyn sisyphus_types::TestExecutor>,
        intervention_manager: Arc<crate::intervention_manager::InterventionManager>,
        internalization_manager: Arc<crate::internalization_manager::InternalizationManager>,
        visual_manager: Option<Arc<sisyphus_visual::VisualPerceptionManager>>,
    ) -> Self {
        Self {
            concurrency_limit: Arc::new(tokio::sync::Semaphore::new(16)), // Default limit: 16 concurrent nodes
            graph_cache: Arc::new(dashmap::DashMap::new()),
            prediction_cache: Arc::new(dashmap::DashMap::new()),
            output_cache: Arc::new(crate::performance::OutputCache::new(1000)),
            test_executor,
            visual_manager,
            intervention_manager,
            internalization_manager,
        }
    }

    /// 执行 SOP 图谱的公开入口。
    /// 返回 BoxFuture 以满足并发调度的 'static Send 要求。
    #[allow(clippy::too_many_arguments)]
    pub fn execute_with_context(
        &self,
        sop_graph: Arc<SOPGraph>,
        _start_node: NodeIndex,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
        supervisor: Option<Arc<dyn RuntimeSupervisor>>,
        micro_reflector: Option<Arc<dyn sisyphus_types::MicroReflector>>,
        visual_manager: Option<Arc<sisyphus_visual::VisualPerceptionManager>>,
    ) -> BoxFuture<'static, Result<Arc<ExecutionContext>, CoreError>> {
        let executor = Arc::new(Executor {
            concurrency_limit: Arc::clone(&self.concurrency_limit),
            graph_cache: Arc::clone(&self.graph_cache),
            prediction_cache: Arc::clone(&self.prediction_cache),
            output_cache: Arc::clone(&self.output_cache),
            test_executor: Arc::clone(&self.test_executor),
            visual_manager: visual_manager.clone(),
            intervention_manager: Arc::clone(&self.intervention_manager),
            internalization_manager: Arc::clone(&self.internalization_manager),
        });
        Box::pin(async move {
            executor
                .execute_impl(
                    sop_graph,
                    _start_node,
                    initial_ctx,
                    event_tx,
                    tool_executor,
                    supervisor,
                    micro_reflector,
                )
                .await
        })
    }

    /// 在后台为后继节点触发 LLM 预测。
    async fn trigger_speculative_predictions(
        &self,
        node: NodeIndex,
        sop_graph: Arc<SOPGraph>,
        ctx: Arc<ExecutionContext>,
        micro_reflector: Option<Arc<dyn sisyphus_types::MicroReflector>>,
    ) {
        let Some(ref reflector) = micro_reflector else {
            return;
        };

        for edge in sop_graph.graph.edges(node) {
            let target = edge.target();
            if let Some(node_ref) = sop_graph.graph.node_weight(target) {
                let name = node_ref.name().to_string();
                let reflector_clone = Arc::clone(reflector);
                let ctx_clone = Arc::clone(&ctx);
                let cache_clone = Arc::clone(&self.prediction_cache);

                tokio::spawn(async move {
                    if let Ok(pred) = reflector_clone.predict(&name, &ctx_clone).await {
                        cache_clone.insert(name, pred);
                    }
                });
            }
        }
    }

    /// DAG 执行逻辑的内部实现。
    #[allow(clippy::too_many_arguments)]
    async fn execute_impl(
        self: Arc<Self>,
        sop_graph: Arc<SOPGraph>,
        _start_node: NodeIndex,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
        supervisor: Option<Arc<dyn RuntimeSupervisor>>,
        micro_reflector: Option<Arc<dyn sisyphus_types::MicroReflector>>,
    ) -> Result<Arc<ExecutionContext>, CoreError> {
        let span = tracing::info_span!(
            "executor.execute",
            graph_nodes = sop_graph.graph.node_count()
        );
        let _enter = span.enter();

        let start = Instant::now();

        // 优化：若已验证则跳过冗余的环检测
        if !sop_graph
            .is_validated
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            if toposort(&sop_graph.graph, None).is_err() {
                tracing::error!("SOPGraph contains cycles");
                return Err(CoreError::CycleDetected);
            }
            sop_graph
                .is_validated
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }

        let ctx = initial_ctx.unwrap_or_else(|| Arc::new(ExecutionContext::new()));

        let mut join_set = JoinSet::new();
        let mut in_degree = HashMap::new();
        let nodes = sop_graph.graph.node_indices();
        for node in nodes {
            let degree = sop_graph
                .graph
                .edges_directed(node, petgraph::Direction::Incoming)
                .count();
            in_degree.insert(node, degree);
        }

        tracing::debug!(
            "Starting DAG execution with {} ready nodes",
            in_degree.values().filter(|&&d| d == 0).count()
        );

        for (&node, &degree) in in_degree.iter() {
            if degree == 0 {
                let node_ref = match sop_graph.graph.node_weight(node) {
                    Some(n) => Arc::clone(n),
                    None => {
                        tracing::error!(node_idx = ?node, "Node not found in graph during execution");
                        continue;
                    }
                };
                let ctx_ref = Arc::clone(&ctx);
                let tx = event_tx.clone();
                let tool_exec = Arc::clone(&tool_executor);
                let _sup = supervisor.clone();
                let sem = Arc::clone(&self.concurrency_limit);
                let exec_self = Arc::clone(&self);
                let micro_ref = micro_reflector.clone();

                join_set.spawn(async move {
                    let name = node_ref.name().to_string();
                    let node_span = tracing::info_span!("executor.node", node = %name);
                    let _enter = node_span.enter();

                    let start_time = Instant::now();
                    let result = async {
                        let _permit = sem.acquire().await.map_err(|e| sisyphus_types::TypesError::NodeExecutionError(format!("Semaphore acquire failed: {e}")))?;

                        if let Some(tx) = tx.as_ref() {
                            if let Err(e) = tx.send(ExecutionEvent::NodeStarted { node_name: name.clone() }) {
                                tracing::warn!(node = %name, error = %e, "Failed to send NodeStarted event");
                            }
                        }

                        exec_self.execute_node_cognitively(
                            node_ref.clone(),
                            ctx_ref.clone(),
                            tx.clone(),
                            tool_exec.clone(),
                            micro_ref.clone(),
                            name.clone()
                        ).await
                    }.await;
                    let duration = start_time.elapsed().as_millis() as i64;

                    if let Some(ref tx) = tx {
                        match &result {
                            Ok(out) => {
                                // P2-9：优先真实测量，回退配置上限（旧执行器兼容）。
                                let fuel = ctx_ref
                                    .take_measurement(&name)
                                    .map(|m| m.fuel_consumed as i64)
                                    .unwrap_or_else(|| {
                                        node_ref
                                            .as_any()
                                            .downcast_ref::<sisyphus_types::WasmSOPNode>()
                                            .map(|wasm| wasm.fuel_limit as i64)
                                            .unwrap_or(0)
                                    });
                                if let Err(e) = tx.send(ExecutionEvent::NodeCompleted {
                                    node_name: name.clone(),
                                    output: out.clone(),
                                    fuel_consumed: fuel,
                                    duration_ms: duration,
                                }) {
                                    tracing::warn!(node = %name, error = %e, "Failed to send NodeCompleted event");
                                }
                            }
                            Err(e) => {
                                // P2-9：优先真实测量，回退配置上限（旧执行器兼容）。
                                let fuel = ctx_ref
                                    .take_measurement(&name)
                                    .map(|m| m.fuel_consumed as i64)
                                    .unwrap_or_else(|| {
                                        node_ref
                                            .as_any()
                                            .downcast_ref::<sisyphus_types::WasmSOPNode>()
                                            .map(|wasm| wasm.fuel_limit as i64)
                                            .unwrap_or(0)
                                    });
                                if let Err(send_err) = tx.send(ExecutionEvent::NodeFailed {
                                    node_name: name.clone(),
                                    error: e.to_string(),
                                    fuel_consumed: fuel,
                                    duration_ms: duration,
                                }) {
                                    tracing::warn!(node = %name, error = %send_err, "Failed to send NodeFailed event");
                                }
                            }
                        }
                    }

                    tracing::debug!(node = %name, "Node execution completed");
                    (node, name, result)
                });
            }
        }

        while let Some(res) = join_set.join_next().await {
            let (node, name, result) = res.map_err(|e| CoreError::Internal(e.to_string()))?;
            let output = match result {
                Ok(out) => out,
                Err(e) => {
                    tracing::warn!(node = %name, error = %e, "Node execution failed");
                    NodeOutput::from(Value::String(e.to_string()))
                }
            };
            ctx.set_result(name.clone(), output.clone());

            self.trigger_speculative_predictions(
                node,
                Arc::clone(&sop_graph),
                Arc::clone(&ctx),
                micro_reflector.clone(),
            )
            .await;

            if let Some(ref sup) = supervisor {
                match sup.evaluate(&name, &output, Arc::clone(&ctx)).await {
                    Ok(SupervisorDecision::Continue { deviation }) => {
                        tracing::debug!(node = %name, deviation, "Supervisor approved execution to continue");
                        ctx.push_trace_point(sisyphus_types::TracePoint {
                            node_name: name.clone(),
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            output_summary: output.to_string(),
                            deviation_score: deviation,
                            status: Some("ok".into()),
                        });
                    }
                    Ok(SupervisorDecision::Pivot { reason, .. }) => {
                        tracing::warn!(node = %name, reason = %reason, "Strategic Pivot detected by supervisor! Aborting all nodes.");
                        ctx.push_trace_point(sisyphus_types::TracePoint {
                            node_name: name.clone(),
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            output_summary: format!("PIVOT: {}", reason),
                            deviation_score: 1.0, // Maximum deviation
                            status: Some("pivot".into()),
                        });
                        join_set.abort_all();
                        return Err(CoreError::StrategicPivot(reason));
                    }
                    Err(e) => {
                        tracing::error!(node = %name, error = %e, "Runtime supervisor evaluation failed");
                    }
                }
            }

            if name.starts_with("Rigorous-") {
                let section_id = name.replace("Rigorous-", "notepad.").to_lowercase();
                ctx.write_notepad(section_id, output.to_string());
                tracing::debug!(node = %name, section = %"notepad", "Auto-synced node output to Notepad");
            }
            tracing::debug!(node = %name, "Node result stored");

            let chosen_target = if let Some(node_weight) = sop_graph.graph.node_weight(node) {
                if let Some(_branch_node) = node_weight
                    .as_any()
                    .downcast_ref::<sisyphus_types::BranchSOPNode>()
                {
                    output.as_f64().map(|idx| NodeIndex::new(idx as usize))
                } else {
                    None
                }
            } else {
                None
            };

            for edge in sop_graph.graph.edges(node) {
                let target = edge.target();

                if let Some(chosen) = chosen_target {
                    if target != chosen {
                        if let Some(degree) = in_degree.get_mut(&target) {
                            *degree -= 1;
                        }
                        continue;
                    }
                }

                if let Some(degree) = in_degree.get_mut(&target) {
                    *degree -= 1;
                    if *degree == 0 {
                        let Some(node_ref) = sop_graph.graph.node_weight(target).map(Arc::clone)
                        else {
                            tracing::error!(node_idx = ?target, "Target node not found in graph after edge traversal");
                            continue;
                        };
                        let ctx_ref = Arc::clone(&ctx);
                        let tx = event_tx.clone();
                        let tool_exec = Arc::clone(&tool_executor);
                        let _sup = supervisor.clone();
                        let sem = Arc::clone(&self.concurrency_limit);
                        let exec_self = Arc::clone(&self);
                        let micro_ref = micro_reflector.clone();
                        join_set.spawn(async move {
                            let name = node_ref.name().to_string();
                            let node_span = tracing::info_span!("executor.node", node = %name);
                            let _enter = node_span.enter();

                            let start_time = Instant::now();
                            let result = async {
                                let _permit = sem.acquire().await.map_err(|e| sisyphus_types::TypesError::NodeExecutionError(format!("Semaphore acquire failed: {e}")))?;

                                if let Some(tx) = tx.as_ref() {
                                    if let Err(e) = tx.send(ExecutionEvent::NodeStarted { node_name: name.clone() }) {
                                        tracing::warn!(node = %name, error = %e, "Failed to send NodeStarted event");
                                    }
                                }

                                exec_self.execute_node_cognitively(
                                    node_ref.clone(),
                                    ctx_ref.clone(),
                                    tx.clone(),
                                    tool_exec.clone(),
                                    micro_ref.clone(),
                                    name.clone()
                                ).await
                            }.await;
                            let duration = start_time.elapsed().as_millis() as i64;

                            if let Some(ref tx) = tx {
                                match &result {
                                    Ok(out) => {
                                        // P2-9：优先真实测量，回退配置上限（旧执行器兼容）。
                                        let fuel = ctx_ref
                                            .take_measurement(&name)
                                            .map(|m| m.fuel_consumed as i64)
                                            .unwrap_or_else(|| {
                                                node_ref
                                                    .as_any()
                                                    .downcast_ref::<sisyphus_types::WasmSOPNode>()
                                                    .map(|wasm| wasm.fuel_limit as i64)
                                                    .unwrap_or(0)
                                            });
                                        if let Err(e) = tx.send(ExecutionEvent::NodeCompleted {
                                            node_name: name.clone(),
                                            output: out.clone(),
                                            fuel_consumed: fuel,
                                            duration_ms: duration,
                                        }) {
                                            tracing::warn!(node = %name, error = %e, "Failed to send NodeCompleted event");
                                        }
                                    }
                                    Err(e) => {
                                        // P2-9：优先真实测量，回退配置上限（旧执行器兼容）。
                                        let fuel = ctx_ref
                                            .take_measurement(&name)
                                            .map(|m| m.fuel_consumed as i64)
                                            .unwrap_or_else(|| {
                                                node_ref
                                                    .as_any()
                                                    .downcast_ref::<sisyphus_types::WasmSOPNode>()
                                                    .map(|wasm| wasm.fuel_limit as i64)
                                                    .unwrap_or(0)
                                            });
                                        if let Err(send_err) = tx.send(ExecutionEvent::NodeFailed {
                                            node_name: name.clone(),
                                            error: e.to_string(),
                                            fuel_consumed: fuel,
                                            duration_ms: duration,
                                        }) {
                                            tracing::warn!(node = %name, error = %send_err, "Failed to send NodeFailed event");
                                        }
                                    }
                                }
                            }

                            tracing::debug!(node = %name, "Node execution completed");
                            (target, name, result)
                        });
                    }
                }
            }

            let duration = start.elapsed();
            tracing::info!(
                duration_ms = duration.as_millis(),
                "DAG execution completed"
            );
        }
        Ok(ctx)
    }

    async fn execute_node_cognitively(
        &self,
        node_ref: Arc<dyn sisyphus_types::SOPNode>,
        ctx: Arc<ExecutionContext>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
        micro_reflector: Option<Arc<dyn sisyphus_types::MicroReflector>>,
        node_name: String,
    ) -> Result<NodeOutput, sisyphus_types::TypesError> {
        let prediction = if let Some(ref reflector) = micro_reflector {
            if let Some(cached_pred) = self.prediction_cache.get(&node_name) {
                Some(cached_pred.clone())
            } else {
                reflector.predict(&node_name, &ctx).await.ok()
            }
        } else {
            None
        };
        let output = if let Some(subgraph_node) = node_ref
            .as_any()
            .downcast_ref::<sisyphus_types::SubGraphSOPNode>()
        {
            let config_key = serde_json::to_string(&subgraph_node.config).map_err(|e| {
                sisyphus_types::TypesError::NodeExecutionError(format!(
                    "Config serialization failed: {e}"
                ))
            })?;
            let subgraph = if let Some(cached) = self.graph_cache.get(&config_key) {
                cached.clone()
            } else {
                let g = sisyphus_types::SOPGraph::from_config(
                    &subgraph_node.config,
                    tool_executor.clone(),
                    self.test_executor.clone(),
                )
                .map_err(|e| sisyphus_types::TypesError::GraphError(e.to_string()))?;
                self.graph_cache.insert(config_key, g.clone());
                g
            };
            let sub_ctx = self
                .execute_with_context(
                    subgraph,
                    petgraph::graph::NodeIndex::new(0),
                    Some(ctx.clone()),
                    event_tx.clone(),
                    tool_executor.clone(),
                    None,
                    micro_reflector.clone(),
                    self.visual_manager.clone(),
                )
                .await
                .map_err(|e| sisyphus_types::TypesError::NodeExecutionError(e.to_string()))?;
            sub_ctx.last_output().ok_or_else(|| {
                sisyphus_types::TypesError::NodeExecutionError(
                    "Sub-graph completed without producing a final output".into(),
                )
            })
        } else {
            node_ref.execute(&ctx).await
        };

        let mut final_output = output?;

        // --- 视觉感知拦截 ---
        if let Value::String(s) = final_output.0.as_ref().clone() {
            if let Some(node_name) = s.strip_prefix("VISUAL_EXEC_REQUIRED:") {
                if let Some(ref v_manager) = self.visual_manager {
                    tracing::info!(node = %node_name, "Intercepted visual execution request");

                    // 从上下文中提取图像数据
                    let image_data = ctx
                        .get_result("current_screenshot")
                        .and_then(|o| o.0.as_str().map(|s| s.as_bytes().to_vec()))
                        .unwrap_or_default();

                    // 使用 VisualSOPNode 的 kind 驱动 VLM
                    if let Some(visual_node) = node_ref
                        .as_any()
                        .downcast_ref::<sisyphus_types::VisualSOPNode>()
                    {
                        let visual_result = v_manager
                            .process_visual_node(&visual_node.kind, &image_data)
                            .await
                            .map_err(|e| {
                                sisyphus_types::TypesError::NodeExecutionError(e.to_string())
                            })?;
                        final_output = NodeOutput::from(Value::String(visual_result));
                    }
                }
            } else if let Some(node_name) = s.strip_prefix("HUMAN_INTERVENTION_REQUIRED:") {
                tracing::info!(node = %node_name, "Intercepted human intervention request");

                // 查找节点配置以获取提示词
                if let Some(interv_node) = node_ref
                    .as_any()
                    .downcast_ref::<sisyphus_types::HumanInterventionSOPNode>()
                {
                    let feedback = self
                        .intervention_manager
                        .request_feedback(
                            node_name.to_string(),
                            interv_node.prompt.clone(),
                            format!("{:?}", ctx),
                            std::time::Duration::from_secs(interv_node.timeout_secs),
                        )
                        .await
                        .map_err(|e| {
                            sisyphus_types::TypesError::NodeExecutionError(e.to_string())
                        })?;

                    final_output = NodeOutput::from(Value::String(feedback.correction.clone()));

                    // --- 触发认知内化 ---
                    let int_manager = Arc::clone(&self.internalization_manager);
                    let int_req = crate::internalization_manager::InternalizationRequest {
                        node_id: node_name.to_string(),
                        prompt: interv_node.prompt.clone(),
                        context: format!("{:?}", ctx),
                        correction: feedback.correction,
                    };
                    tokio::spawn(async move {
                        if let Err(e) = int_manager.internalize_feedback(int_req).await {
                            tracing::error!(error = %e, "Cognitive internalization failed for node {}", s);
                        }
                    });
                }
            }
        }

        if let (Some(ref reflector), Some(pred)) = (&micro_reflector, prediction) {
            if node_ref.reflection_priority() == sisyphus_types::ReflectionPriority::None {
                tracing::debug!(node = %node_name, "Skipping reflection due to None priority");
                return Ok(final_output);
            }

            let actual = final_output.to_string();
            match reflector.reflect(&node_name, &pred, &actual, &ctx).await {
                Ok(sisyphus_types::MicroReflectionDecision::Continue) => {
                    tracing::debug!(node = %node_name, "Micro-reflection: Continue");
                    if let Some(ref tx) = event_tx {
                        let _ = tx.send(ExecutionEvent::MicroReflection {
                            node_name: node_name.clone(),
                            prediction: pred.clone(),
                            actual: actual.clone(),
                            decision: "CONTINUE".into(),
                            updates: None,
                            reason: None,
                        });
                    }
                }
                Ok(sisyphus_types::MicroReflectionDecision::Adjust { updates }) => {
                    tracing::info!(node = %node_name, updates = ?updates, "Micro-reflection: Adjusting context");
                    if let Some(ref tx) = event_tx {
                        let _ = tx.send(ExecutionEvent::MicroReflection {
                            node_name: node_name.clone(),
                            prediction: pred.clone(),
                            actual: actual.clone(),
                            decision: "ADJUST".into(),
                            updates: Some(updates.clone()),
                            reason: None,
                        });
                    }
                    for (k, v) in updates {
                        ctx.set_result(k, NodeOutput::from(v));
                    }
                }
                Ok(sisyphus_types::MicroReflectionDecision::Pivot { reason }) => {
                    tracing::warn!(node = %node_name, reason = %reason, "Micro-reflection: Strategic Pivot requested!");
                    if let Some(ref tx) = event_tx {
                        let _ = tx.send(ExecutionEvent::MicroReflection {
                            node_name: node_name.clone(),
                            prediction: pred.clone(),
                            actual: actual.clone(),
                            decision: "PIVOT".into(),
                            updates: None,
                            reason: Some(reason.clone()),
                        });
                    }
                    return Err(sisyphus_types::TypesError::NodeExecutionError(format!(
                        "PIVOT: {}",
                        reason
                    )));
                }
                Err(e) => {
                    tracing::error!(node = %node_name, error = %e, "Micro-reflection failed");
                }
            }
        }
        Ok(final_output)
    }

    pub async fn execute(
        &self,
        sop_graph: Arc<SOPGraph>,
        start_node: NodeIndex,
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
    ) -> Result<Arc<ExecutionContext>, CoreError> {
        self.execute_with_context(
            sop_graph,
            start_node,
            None,
            None,
            tool_executor,
            None,
            None,
            None,
        )
        .await
    }
}
