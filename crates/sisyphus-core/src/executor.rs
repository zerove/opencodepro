//! DAG 执行器 — 拓扑感知的节点调度与事件流。

use futures::future::BoxFuture;
use petgraph::algo::toposort;
use petgraph::graph::NodeIndex;
use petgraph::visit::{Dfs, EdgeRef};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::Hasher;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::{CoreError, ExecutionContext, ExecutionEvent, NodeOutput, SOPGraph};
use sisyphus_types::{RuntimeSupervisor, SupervisorDecision};

/// 将序列化输出直接喂给 `Hasher` 的 `Write` 适配器 — 用于在不分配
/// 完整 JSON 字符串的情况下生成缓存键。
struct HashWriter<'a, H: Hasher>(&'a mut H);

impl<H: Hasher> std::io::Write for HashWriter<'_, H> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 基于 DAG 的执行器，以并行节点调度运行 SOP 图谱，
/// 并支持可选的事件流。
pub struct Executor {
    pub concurrency_limit: Arc<tokio::sync::Semaphore>,
    /// 当前配置的并发上限。`Semaphore::available_permits()` 会被在途节点
    /// 占用，不能当作上限使用，因此单独跟踪目标值以支持热重载。
    configured_limit: Arc<std::sync::atomic::AtomicUsize>,
    /// 上次缩减上限时因许可被占用而没能回收的数量（见 [`Executor::apply_config`]）。
    pending_reduction: Arc<std::sync::atomic::AtomicUsize>,
    pub graph_cache: Arc<dashmap::DashMap<String, Arc<SOPGraph>>>,
    pub prediction_cache: Arc<dashmap::DashMap<String, String>>,
    pub output_cache: Arc<crate::performance::OutputCache>,
    pub test_executor: Arc<dyn sisyphus_types::TestExecutor>,
    pub visual_manager: Option<Arc<sisyphus_visual::VisualPerceptionManager>>,
    pub intervention_manager: Arc<crate::intervention_manager::InterventionManager>,
    pub internalization_manager: Arc<crate::internalization_manager::InternalizationManager>,
}

impl Executor {
    /// New.
    pub fn new(
        test_executor: Arc<dyn sisyphus_types::TestExecutor>,
        intervention_manager: Arc<crate::intervention_manager::InterventionManager>,
        internalization_manager: Arc<crate::internalization_manager::InternalizationManager>,
        visual_manager: Option<Arc<sisyphus_visual::VisualPerceptionManager>>,
    ) -> Self {
        Self {
            concurrency_limit: Arc::new(tokio::sync::Semaphore::new(16)), // Default limit: 16 concurrent nodes
            configured_limit: Arc::new(std::sync::atomic::AtomicUsize::new(16)),
            pending_reduction: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            graph_cache: Arc::new(dashmap::DashMap::new()),
            prediction_cache: Arc::new(dashmap::DashMap::new()),
            output_cache: Arc::new(crate::performance::OutputCache::new(1000)),
            test_executor,
            visual_manager,
            intervention_manager,
            internalization_manager,
        }
    }

    /// 运行时应用执行器配置（热重载）。
    ///
    /// 通过增删 [`Semaphore`] 的许可数调整并发上限，**不替换** `Arc`：
    /// 已经在执行的批次持有的是同一个闸门，因此热重载对在途执行同样生效
    /// （旧实现只打一行 “restart required” 日志，配置根本不生效）。
    ///
    /// 缩减上限时若部分许可正被在途节点占用，[`Semaphore::forget_permits`]
    /// 无法立即回收它们；差额会记在 `pending_reduction` 上，由后续
    /// `apply_config` 调用继续回收，直到与目标值一致。
    pub fn apply_config(&self, executor_config: &sisyphus_types::ExecutorConfig) {
        let target = executor_config.max_concurrent_tasks.max(1);
        let previous = self.configured_limit.swap(target, std::sync::atomic::Ordering::AcqRel);
        let pending = self.pending_reduction.swap(0, std::sync::atomic::Ordering::AcqRel);

        // 闸门的实际容量 = 已配置上限 + 上次没能回收的缩减量。
        let nominal = previous.saturating_add(pending);
        if target > nominal {
            self.concurrency_limit.add_permits(target - nominal);
        } else if target < nominal {
            let want = nominal - target;
            let removed = self.concurrency_limit.forget_permits(want);
            let leftover = want.saturating_sub(removed);
            if leftover > 0 {
                self.pending_reduction.store(leftover, std::sync::atomic::Ordering::Release);
                tracing::info!(
                    target,
                    pending = leftover,
                    "Executor concurrency limit reduced partially; \
                     remaining permits are held by in-flight nodes"
                );
            }
        }

        tracing::info!(
            max_concurrent_tasks = target,
            default_fuel = executor_config.default_fuel,
            "Executor config applied (concurrency limit updated in place)"
        );
    }

    /// 当前配置的并发上限（不等于 `Semaphore::available_permits()`：
    /// 后者会被在途节点占用）。
    pub fn configured_limit(&self) -> usize {
        self.configured_limit.load(std::sync::atomic::Ordering::Acquire)
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
            configured_limit: Arc::clone(&self.configured_limit),
            pending_reduction: Arc::clone(&self.pending_reduction),
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
                // 推测预测先查缓存：已有预测则跳过，避免重复 LLM 调用。
                if self.prediction_cache.contains_key(&name) {
                    continue;
                }
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
        if !sop_graph.is_validated.load(std::sync::atomic::Ordering::SeqCst) {
            if toposort(&sop_graph.graph, None).is_err() {
                tracing::error!("SOPGraph contains cycles");
                return Err(CoreError::CycleDetected);
            }
            sop_graph.is_validated.store(true, std::sync::atomic::Ordering::SeqCst);
        }

        let ctx = initial_ctx.unwrap_or_else(|| Arc::new(ExecutionContext::new()));

        let mut join_set = JoinSet::new();
        let mut in_degree = HashMap::new();
        let nodes = sop_graph.graph.node_indices();
        for node in nodes {
            let degree =
                sop_graph.graph.edges_directed(node, petgraph::Direction::Incoming).count();
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
                    },
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
                        let permit = sem.acquire().await.map_err(|e| sisyphus_types::TypesError::NodeExecutionError(format!("Semaphore acquire failed: {e}")))?;

                        if let Some(tx) = tx.as_ref() {
                            if let Err(e) = tx.send(ExecutionEvent::NodeStarted { node_name: name.clone() }) {
                                tracing::warn!(node = %name, error = %e, "Failed to send NodeStarted event");
                            }
                        }

                        // P0 ③ 并发不变量：子图节点在等待子图执行前释放并发许可，
                        // 因为子节点需从同一信号量取许可；父节点持许可等待子节点
                        // 会形成 hold-and-wait 死锁（≥16 个并发子图时必现）。
                        if node_ref.as_any().is::<sisyphus_types::SubGraphSOPNode>() {
                            drop(permit);
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
            let (output, failure) = match result {
                Ok(out) => (out, None),
                Err(e) => {
                    tracing::warn!(node = %name, error = %e, "Node execution failed");
                    // 字符串仅供既有分支和监督器读取，不再充当成功凭据。
                    (NodeOutput::from(Value::String(e.to_string())), Some(e))
                },
            };
            ctx.set_result(name.clone(), output.clone());

            if let Some(ref sup) = supervisor {
                // 必须先记录当前节点，再让 supervisor 评估。
                // supervisor 通过 `ctx.trace` 重建执行轨迹来做偏差判定，
                // 旧顺序（先 evaluate 后 push）导致 evaluate 时 trace 恒为空
                // ——首个节点必然如此——`StandardReflector::parse_trace` 得不到
                // 任何记录，走无记录兜底分支（reflect.rs:735）把**成功**执行
                // 判成 0.1 偏差，越过 High 档 0.01 阈值后无条件 pivot。
                // 实测 sisyphus-tests 的 5 个集成用例（test_full_pipeline、
                // test_dag_execution、test_cognitive_loop、
                // test_wasm_tool_chain、test_concurrent_execution）即以此失败。
                ctx.push_trace_point(sisyphus_types::TracePoint {
                    node_name: name.clone(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    output_summary: output.to_string(),
                    deviation_score: 0.0,
                    status: Some(if failure.is_some() { "error" } else { "ok" }.into()),
                });

                match sup.evaluate(&name, &output, Arc::clone(&ctx)).await {
                    Ok(SupervisorDecision::Continue { deviation }) => {
                        tracing::debug!(node = %name, deviation, "Supervisor approved execution to continue");
                        // 轨迹点已在 evaluate 之前写入（见上），此处只回填
                        // supervisor 判定的偏差值，避免同节点出现两条记录。
                        if let Ok(mut trace) = ctx.trace.write() {
                            if let Some(point) = trace.iter_mut().rev().find(|p| {
                                p.node_name == name && p.status.as_deref() != Some("pivot")
                            }) {
                                point.deviation_score = deviation;
                            }
                        }
                    },
                    Ok(SupervisorDecision::Pivot { reason, .. }) => {
                        tracing::warn!(node = %name, reason = %reason, "Strategic Pivot detected by supervisor! Aborting all nodes.");
                        // 就地改写 evaluate 之前写入的那条记录：把状态标记为
                        // pivot 并记录原因，不追加第二条（同上，避免重复）。
                        let marked = if let Ok(mut trace) = ctx.trace.write() {
                            trace
                                .iter_mut()
                                .rev()
                                .find(|p| {
                                    p.node_name == name && p.status.as_deref() != Some("pivot")
                                })
                                .map(|p| {
                                    p.output_summary = format!("PIVOT: {}", reason);
                                    p.deviation_score = 1.0; // Maximum deviation
                                    p.status = Some("pivot".into());
                                })
                                .is_some()
                        } else {
                            false
                        };
                        if !marked {
                            // 理论上不会发生（evaluate 前刚写入）；保底补写以免丢失 pivot 事实。
                            ctx.push_trace_point(sisyphus_types::TracePoint {
                                node_name: name.clone(),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                output_summary: format!("PIVOT: {}", reason),
                                deviation_score: 1.0,
                                status: Some("pivot".into()),
                            });
                        }
                        join_set.abort_all();
                        return Err(CoreError::StrategicPivot(reason));
                    },
                    Err(e) => {
                        tracing::error!(node = %name, error = %e, "Runtime supervisor evaluation failed");
                    },
                }
            }

            if let Some(source) = failure {
                // BranchSOPNode 以先前输出（包括错误文本）决定恢复路径。
                // 仅阻断确定没有下游分支入口的失败，避免破坏已有直接或间接恢复。
                // 可达分支不等于已恢复。完整恢复判定需要独立的图级状态协议，
                // 本次定点修复不改变这部分既有语义。
                let mut descendants = Dfs::new(&sop_graph.graph, node);
                let mut has_recovery_branch = false;
                while let Some(next) = descendants.next(&sop_graph.graph) {
                    if next != node
                        && sop_graph.graph[next].as_any().is::<sisyphus_types::BranchSOPNode>()
                    {
                        has_recovery_branch = true;
                        break;
                    }
                }
                if !has_recovery_branch {
                    join_set.abort_all();
                    return Err(CoreError::NodeExecutionFailed {
                        node_name: name,
                        source: Box::new(source),
                    });
                }
            }

            self.trigger_speculative_predictions(
                node,
                Arc::clone(&sop_graph),
                Arc::clone(&ctx),
                micro_reflector.clone(),
            )
            .await;

            if name.starts_with("Rigorous-") {
                let section_id = name.replace("Rigorous-", "notepad.").to_lowercase();
                ctx.write_notepad(section_id, output.to_string());
                tracing::debug!(node = %name, section = %"notepad", "Auto-synced node output to Notepad");
            }
            tracing::debug!(node = %name, "Node result stored");

            let chosen_target = if let Some(node_weight) = sop_graph.graph.node_weight(node) {
                if node_weight.as_any().downcast_ref::<sisyphus_types::BranchSOPNode>().is_some() {
                    // P1 修复：分支决策必须解析为真实出边目标。旧行为把任意
                    // 数值（NaN、越界、非整数）静默转成 NodeIndex；匹配不到
                    // 出边时会递减全部出边且不放行任何节点 —— 调度静默停摆。
                    let raw = output.as_f64().ok_or_else(|| CoreError::NodeExecutionFailed {
                        node_name: name.to_string(),
                        source: Box::new(sisyphus_types::TypesError::NodeExecutionError(format!(
                            "branch node produced non-numeric decision: {output}"
                        ))),
                    })?;
                    if !raw.is_finite() || raw < 0.0 || raw.fract() != 0.0 {
                        return Err(CoreError::NodeExecutionFailed {
                            node_name: name.to_string(),
                            source: Box::new(sisyphus_types::TypesError::NodeExecutionError(format!(
                                "branch node produced invalid decision {raw} (must be a non-negative integer)"
                            ))),
                        });
                    }
                    let candidate = NodeIndex::new(raw as usize);
                    if !sop_graph.graph.edges(node).any(|e| e.target() == candidate) {
                        return Err(CoreError::NodeExecutionFailed {
                            node_name: name.to_string(),
                            source: Box::new(sisyphus_types::TypesError::NodeExecutionError(
                                format!(
                                    "branch decision {} does not match any outgoing edge",
                                    raw as usize
                                ),
                            )),
                        });
                    }
                    Some(candidate)
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
                                let permit = sem.acquire().await.map_err(|e| sisyphus_types::TypesError::NodeExecutionError(format!("Semaphore acquire failed: {e}")))?;

                                if let Some(tx) = tx.as_ref() {
                                    if let Err(e) = tx.send(ExecutionEvent::NodeStarted { node_name: name.clone() }) {
                                        tracing::warn!(node = %name, error = %e, "Failed to send NodeStarted event");
                                    }
                                }

                                // P0 ③ 并发不变量：子图节点在等待子图执行前释放并发许可，
                                // 因为子节点需从同一信号量取许可；父节点持许可等待子节点
                                // 会形成 hold-and-wait 死锁（≥16 个并发子图时必现）。
                                if node_ref.as_any().is::<sisyphus_types::SubGraphSOPNode>() {
                                    drop(permit);
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
        let output = if let Some(subgraph_node) =
            node_ref.as_any().downcast_ref::<sisyphus_types::SubGraphSOPNode>()
        {
            // 缓存键取配置内容的哈希：直接序列化进哈希器，避免为每次
            // 节点执行分配完整 JSON 字符串（配置可能很大）。
            let mut hasher = DefaultHasher::new();
            serde_json::to_writer(HashWriter(&mut hasher), &subgraph_node.config).map_err(|e| {
                sisyphus_types::TypesError::NodeExecutionError(format!(
                    "Config serialization failed: {e}"
                ))
            })?;
            let config_key = format!("{:016x}", hasher.finish());
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
                    if let Some(visual_node) =
                        node_ref.as_any().downcast_ref::<sisyphus_types::VisualSOPNode>()
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
                if let Some(interv_node) =
                    node_ref.as_any().downcast_ref::<sisyphus_types::HumanInterventionSOPNode>()
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
                },
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
                },
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
                },
                Err(e) => {
                    tracing::error!(node = %node_name, error = %e, "Micro-reflection failed");
                },
            }
        }
        Ok(final_output)
    }

    /// Execute.
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
