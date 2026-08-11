//! 认知循环执行：预测（predict）→ 执行（execute）→ 反思（reflect）→ 演进（evolve）的循环。
//!
//! [`run_cognitive_loop`](StrategicManager::run_cognitive_loop) 编排整个循环；
//! 每个阶段被拆分为独立的辅助函数，使编排逻辑保持清晰，
//! 且每个阶段都可以独立演进。

use std::sync::Arc;
use std::time::{Instant, SystemTime};

use petgraph::graph::NodeIndex;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{error, instrument, Instrument};

use sisyphus_intel::reflect::ReflectionReport;
use sisyphus_intel::{SimulationResult, SisyphusSupervisor};
use sisyphus_mem::MfsStore;
use sisyphus_types::SOPGraph;
use sisyphus_types::{CognitiveEvent, CognitiveMetrics, ExecutionTrace};

use crate::cognitive_loop_v2::AnomalyDetector;
use crate::{CoreError, ExecutionContext, ExecutionEvent, ExecutionStatus, Executor, NodeOutput};

// OpenTelemetry span helpers
use sisyphus_metrics::observability::{cognitive_loop_span, evolution_span, reflection_span};

use super::{CognitiveIteration, CognitiveReport, StrategicManager};

impl StrategicManager {
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip_all, fields(sop_id = %sop_id, graph_nodes = sop_graph.graph.node_count()))]
    pub async fn run_cognitive_loop(
        &self,
        sop_graph: Arc<SOPGraph>,
        sop_id: &str,
        goal_text: Option<&str>,
        project_id: Option<&str>,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        broadcast_fn: impl Fn(&str, ExecutionEvent),
        stream_tx: Option<mpsc::UnboundedSender<CognitiveEvent>>,
        micro_reflector: Option<Arc<dyn sisyphus_types::MicroReflector>>,
    ) -> Result<(NodeOutput, CognitiveReport, Option<Arc<SOPGraph>>), CoreError> {
        let _start_time = std::time::Instant::now();

        // --- 热启动阶段（HOT START） ---
        let (mut current_ref, hot_start_active) = self
            .hot_start_phase(&sop_graph, goal_text, project_id, &stream_tx)
            .await;

        let (_current_strategy, _current_params) = {
            let context = serde_json::json!({
                "sop_id": sop_id,
                "node_count": current_ref.graph.node_count(),
            });
            let ml = self.meta_learner.lock();
            ml.best_strategy_params(&context)
                .unwrap_or_else(|| ("balanced".to_string(), serde_json::json!({})))
        };

        let (max_iterations, convergence_threshold) = {
            // --- 动态深度调整（v2） ---
            let depth = sisyphus_intel::ComplexityAnalyzer::analyze(
                self.experience_bank.clone(),
                &current_ref,
                goal_text.unwrap_or(sop_id),
            )
            .await;

            let (mut d_max, d_thresh) = depth.to_config();

            // 如果热启动生效，可以大幅减少迭代次数
            if hot_start_active {
                d_max = (d_max / 2).max(2);
            }

            // v2：同时获取 AdaptiveDepthController 的预算
            let v2_budget = self
                .depth_controller
                .initial_budget(
                    self.experience_bank.clone(),
                    &current_ref,
                    goal_text.unwrap_or(sop_id),
                )
                .await;
            let final_max = d_max.min(v2_budget as u32);

            let mut config_lock = self.config.write();
            tracing::info!(
                "Updating loop parameters: max_iterations {} -> {} (v2: {}), threshold -> {}",
                config_lock.max_iterations,
                final_max,
                v2_budget,
                d_thresh
            );

            // 用动态深度覆盖策略参数
            config_lock.max_iterations = final_max;
            config_lock.convergence_threshold = d_thresh;

            (
                config_lock.max_iterations as usize,
                config_lock.convergence_threshold,
            )
        };

        let span = tracing::info_span!("strategic.run_cognitive_loop", sop_id = %sop_id, graph_nodes = sop_graph.graph.node_count());
        let _enter = span.enter();

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.execution_manager.active_executions.insert(
            sop_id.to_string(),
            ExecutionStatus {
                goal_id: sop_id.to_string(),
                status: "Initializing".into(),
                current_node: None,
                thought_logs: vec!["Starting cognitive loop...".into()],
                progress: 0.0,
                last_updated: now,
            },
        );

        broadcast_fn(
            sop_id,
            ExecutionEvent::CognitiveStep {
                step: "Initializing".into(),
                message: "Starting cognitive loop...".into(),
                metrics: Some(CognitiveMetrics {
                    iteration: 0,
                    deviation: 0.0,
                    confidence: 0.0,
                    progress: 0.0,
                    token_cost: 0,
                    stability: 1.0,
                    info_gain: 0.0,
                }),
            },
        );

        let executor = Executor::new(
            Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
            self.intervention_manager.clone(),
            self.internalization_manager.clone(),
            self.visual_manager.clone(),
        );
        let mut current_start = sop_graph.start_node;
        let mut iterations: Vec<CognitiveIteration> = Vec::with_capacity(max_iterations);
        let _traces: Vec<ExecutionTrace> = Vec::with_capacity(max_iterations);
        let mut final_result = NodeOutput::default();
        let mut final_evolved: Option<Arc<SOPGraph>> = None;
        let initial_ctx_for_iter = initial_ctx;

        for iteration in 0..max_iterations {
            let iter_start = Instant::now();
            let iter_span = tracing::info_span!("cognitive_iteration", iteration, sop_id = %sop_id);
            let _iter_enter = iter_span.enter();

            let ctx = ExecutionContext::with_capacity(current_ref.graph.node_count());
            let iter_now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // 1. 预测（Predict）
            let predict_span = cognitive_loop_span(sop_id, iteration);
            let (prediction, mut pivot_reason) = self
                .predict_phase(
                    &current_ref,
                    &ctx,
                    sop_id,
                    goal_text,
                    project_id,
                    iteration,
                    max_iterations,
                    iter_now,
                    &broadcast_fn,
                    &stream_tx,
                )
                .instrument(predict_span)
                .await?;

            let exec_ctx = if iteration == 0 {
                initial_ctx_for_iter.clone()
            } else {
                None
            };

            // 2. 执行（Execute）
            let exec_span = tracing::info_span!("execute_phase", sop_id = %sop_id, iteration);
            let (final_ctx, result) = self
                .execute_phase(
                    &executor,
                    &current_ref,
                    current_start,
                    exec_ctx,
                    &prediction,
                    sop_id,
                    goal_text,
                    iteration,
                    max_iterations,
                    convergence_threshold,
                    micro_reflector.clone(),
                    &event_tx,
                    &broadcast_fn,
                    &stream_tx,
                    &mut pivot_reason,
                )
                .instrument(exec_span)
                .await?;

            final_result = result.clone();

            // 3. 反思（Reflect）
            let reflect_span = reflection_span(sop_id, iteration);
            let reflection = self
                .reflect_phase(
                    &current_ref,
                    &final_ctx,
                    &prediction,
                    &pivot_reason,
                    sop_id,
                    iteration,
                    max_iterations,
                    convergence_threshold,
                    goal_text,
                    &stream_tx,
                )
                .instrument(reflect_span)
                .await?;

            // 4. 演进（Evolve）
            let evolve_span = evolution_span(sop_id, iteration);
            let evolved_graph = self
                .evolve_phase(
                    &ctx,
                    &current_ref,
                    &reflection,
                    &prediction,
                    sop_id,
                    goal_text,
                )
                .instrument(evolve_span)
                .await;

            // 4.5 在采纳演进结果之前先做验证
            let final_evolved_graph = if let Some(eg) = evolved_graph {
                self.verify_evolution(
                    eg,
                    &ctx,
                    &prediction,
                    sop_id,
                    project_id,
                    goal_text,
                    iteration,
                    &stream_tx,
                )
                .await
            } else {
                None
            };

            let has_evolved = final_evolved_graph.is_some();
            let _iter_duration = iter_start.elapsed();
            let confidence = 1.0 - reflection.deviation_score.min(1.0);
            iterations.push(CognitiveIteration {
                iteration,
                prediction: prediction.clone(),
                deviation_score: reflection.deviation_score,
                suggestions: reflection.suggested_improvements.clone(),
                evolved: has_evolved,
            });

            // v2：将置信度反馈给 AdaptiveDepthController
            let should_continue = self.depth_controller.observe_iteration(confidence);

            // v2：检查异常报告是否触发额外反思
            let anomaly_report = self.anomaly_detector.generate_report();
            if anomaly_report.should_extra_reflect {
                tracing::warn!(
                    anomalies = anomaly_report.anomalies.len(),
                    severity = ?anomaly_report.overall_severity,
                    "V2 AnomalyDetector triggered extra reflection"
                );
                if let Some(ref tx) = stream_tx {
                    let _ = tx.send(CognitiveEvent::CognitiveStep {
                        step: "AnomalyDetected".into(),
                        message: format!(
                            "{} anomalies (severity: {:?})",
                            anomaly_report.anomalies.len(),
                            anomaly_report.overall_severity,
                        ),
                        metrics: Some(CognitiveMetrics {
                            iteration,
                            deviation: reflection.deviation_score,
                            confidence,
                            progress: (iteration as f32 / max_iterations as f32) * 100.0,
                            token_cost: 0,
                            stability: prediction.predicted_stability,
                            info_gain: 0.0,
                        }),
                    });
                }
            }

            if (reflection.deviation_score < convergence_threshold || !has_evolved)
                && !anomaly_report.should_extra_reflect
            {
                if !has_evolved && reflection.deviation_score >= convergence_threshold {
                    // --- 工具缺口检测 ---
                    if let Ok(Some(gap)) = self
                        .gap_analyzer
                        .detect_gap(&reflection, &current_ref, goal_text.unwrap_or(sop_id))
                        .await
                    {
                        tracing::warn!(gap_id = %gap.gap_id, "Tool gap detected! Capability missing: {}", gap.required_capability);

                        if let Some(ref tx) = stream_tx {
                            let _ = tx.send(CognitiveEvent::ToolGapDetected {
                                gap: serde_json::to_value(&gap).unwrap_or_default(),
                            });
                        }

                        // --- 触发自编程循环 ---
                        let sp_manager = Arc::clone(&self.sp_manager);
                        tokio::spawn(async move {
                            if let Err(e) = sp_manager.resolve_gap(gap).await {
                                error!("Self-programming loop failed: {e}");
                            }
                        });
                    }
                }
                final_evolved = final_evolved_graph;
                break;
            }

            // v2：深度控制器建议停止时也跳出循环
            if !should_continue && reflection.deviation_score < convergence_threshold * 1.5 {
                tracing::info!(
                    iteration,
                    confidence,
                    "AdaptiveDepthController: stopping early (convergence plateau)"
                );
                final_evolved = final_evolved_graph;
                break;
            }

            if let Some(eg) = final_evolved_graph {
                current_start = eg.start_node;
                current_ref = eg;
            } else {
                break;
            }

            // 更新认知图谱（Cognitive Map）用于可视化
            let cog_map = Self::build_cog_map(
                sop_id,
                goal_text.unwrap_or(sop_id),
                &current_ref,
                &final_ctx,
                iteration,
                None, // Active node would be the one currently being executed
            )?;
            self.execution_manager
                .cognitive_maps
                .insert(sop_id.to_string(), cog_map);

            // 战略转移分析：检查语义失败簇
            if let Ok(Some(cluster)) = self
                .pattern_analyzer
                .analyze_failure_clusters(
                    self.experience_bank.as_ref(),
                    goal_text.unwrap_or(sop_id),
                )
                .await
            {
                if let Some(ref tx) = stream_tx {
                    let _ = tx.send(CognitiveEvent::StrategicShift {
                        analysis: serde_json::json!({
                            "cluster": cluster.goal_cluster,
                            "mode": cluster.common_failure_mode,
                            "count": cluster.evidence_count,
                            "suggestion": cluster.suggested_shift
                        }),
                    });
                }

                // 作为战略提案正式提交给路线图
                let _ = self
                    .roadmap_manager
                    .submit_strategic_shift(
                        project_id.unwrap_or("default"),
                        &cluster.common_failure_mode,
                        cluster.evidence_count,
                        &cluster.suggested_shift,
                    )
                    .await;

                tracing::warn!(
                    cluster = %cluster.goal_cluster,
                    mode = %cluster.common_failure_mode,
                    "Strategic failure cluster detected! Proposal submitted: {}", cluster.suggested_shift
                );
            }
        }

        let converged = iterations
            .last()
            .map(|i| i.deviation_score < convergence_threshold)
            .unwrap_or(false);

        // 推送完成事件
        if let Some(ref tx) = stream_tx {
            let _ = tx.send(CognitiveEvent::Complete {
                final_result: final_result.clone(),
                converged,
            });
        }

        let report_obj = CognitiveReport {
            iterations: iterations.len(),
            converged,
            final_deviation: iterations.last().map(|i| i.deviation_score).unwrap_or(0.0),
            history: iterations,
            traces: vec![], // Simplified for rewrite
        };

        Ok((final_result, report_obj, final_evolved))
    }

    /// 热启动阶段：从多模态经验库中检索相关历史图谱，
    /// 作为认知循环的起始点。
    async fn hot_start_phase(
        &self,
        sop_graph: &Arc<SOPGraph>,
        goal_text: Option<&str>,
        project_id: Option<&str>,
        stream_tx: &Option<mpsc::UnboundedSender<CognitiveEvent>>,
    ) -> (Arc<SOPGraph>, bool) {
        let mut current_ref = Arc::clone(sop_graph);
        let mut hot_start_active = false;

        if let Some(goal) = goal_text {
            // 1. 视觉嵌入检索
            let mut visual_embedding = None;
            if let Some(ref v_manager) = self.visual_manager {
                // 在真实系统中，我们会从执行器/操作系统捕获当前截图。
                // 目前我们用基于目标派生的确定性视觉状态来模拟，
                // 使不同目标映射到不同的状态。
                let dummy_image = sisyphus_visual::simulated_visual_state(goal);
                if let Ok(emb) = v_manager.get_embedding(&dummy_image).await {
                    visual_embedding = Some(emb);
                }
            }

            let query = sisyphus_types::experience::ExperienceQuery {
                query_text: goal.to_string(),
                context_hash: format!("{:x}", md5::compute(goal)),
                project_id: project_id.map(|s| s.to_string()),
                action_id: None,
                visual_embedding,
                limit: 5,
            };

            if let Ok(Some(_asset)) = self
                .sp_manager
                .knowledge_manager
                .find_best_asset(goal, self.intelligence_manager.llm_client.clone())
                .await
            {
                // 我们也考虑多模态经验库，以获得更细致的热启动。
                // （实际上 find_best_asset 是更高层的抽象，多模态检索直接使用 experience_bank）
            }

            // 直接进行多模态经验检索
            if let Ok(exps) = self.experience_bank.query_similar(query).await {
                if let Some(best_exp) = exps.into_iter().min_by(|a, b| {
                    a.deviation_score
                        .partial_cmp(&b.deviation_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }) {
                    tracing::info!(goal = %goal, asset_id = %best_exp.action_id, "Multi-modal hot-start match found!");
                    if let Ok(graph) = self
                        .execution_manager
                        .load_sop(
                            &best_exp.action_id,
                            best_exp.project_id.as_deref().unwrap_or("default"),
                        )
                        .await
                    {
                        current_ref = graph;
                        hot_start_active = true;
                    }
                }
            }

            if let Some(ref tx) = stream_tx {
                let _ = tx.send(CognitiveEvent::HotStart {
                    asset_id: "multi-modal-bank".into(),
                    confidence: 1.0,
                });
            }

            // 将认知事件流绑定到自编程管理器以便观测
            if let Some(ref tx) = stream_tx {
                self.sp_manager.set_event_sender(tx.clone());
            }
            // -----------------------
        }
        // -----------------------

        (current_ref, hot_start_active)
    }

    /// 阶段 1（预测）：模拟潜在执行路径，审计逻辑
    /// 一致性，用概率规划器对路径排序，并发送事件。
    #[allow(clippy::too_many_arguments)]
    async fn predict_phase(
        &self,
        current_ref: &Arc<SOPGraph>,
        ctx: &ExecutionContext,
        sop_id: &str,
        goal_text: Option<&str>,
        project_id: Option<&str>,
        iteration: usize,
        max_iterations: usize,
        iter_now: u64,
        broadcast_fn: &impl Fn(&str, ExecutionEvent),
        stream_tx: &Option<mpsc::UnboundedSender<CognitiveEvent>>,
    ) -> Result<(SimulationResult, Option<String>), CoreError> {
        let mut pivot_reason: Option<String> = None;

        // 1. 预测（Predict）
        tracing::info!(iteration, "phase=predict");
        let depth = sisyphus_intel::ComplexityAnalyzer::analyze(
            self.experience_bank.clone(),
            current_ref,
            goal_text.unwrap_or(sop_id),
        )
        .await;

        let prediction = if !depth.should_simulate() {
            tracing::info!(sop_id, "Skipping simulation phase due to low complexity");
            sisyphus_intel::simulation::SimulationResult {
                success_probability: 1.0,
                predicted_stability: 1.0,
                path_id: "default".into(),
                estimated_cost: 0,
                predicted_effort: 0,
                predicted_outcome: "success".into(),
            }
        } else {
            if let Some(mut status) = self.execution_manager.active_executions.get_mut(sop_id) {
                status.status = "Predicting".into();
                status.thought_logs.push(format!(
                    "Iteration {}: Simulating potential paths...",
                    iteration
                ));
                status.last_updated = iter_now;
            }
            broadcast_fn(
                sop_id,
                ExecutionEvent::CognitiveStep {
                    step: "Predicting".into(),
                    message: format!("Iteration {}: Simulating potential paths...", iteration),
                    metrics: Some(CognitiveMetrics {
                        iteration,
                        deviation: 0.0,
                        confidence: 0.0,
                        progress: (iteration as f32 / max_iterations as f32) * 100.0,
                        token_cost: 0,
                        stability: 1.0,
                        info_gain: 0.0,
                    }),
                },
            );
            self.intelligence_manager
                .intel
                .simulator
                .simulate(
                    Arc::clone(current_ref),
                    ctx,
                    project_id.unwrap_or("default"),
                    sop_id,
                    goal_text,
                )
                .instrument(tracing::info_span!("simulate", iteration))
                .await
                .map_err(|e| CoreError::Internal(e.to_string()))?
        };

        // 1.5 逻辑仿真（白盒）
        if let Some(mut status) = self.execution_manager.active_executions.get_mut(sop_id) {
            status.status = "Simulating Logic".into();
            status.thought_logs.push(format!(
                "Iteration {}: Auditing logical consistency...",
                iteration
            ));
            status.last_updated = iter_now;
        }
        let log_sim_result = self
            .intelligence_manager
            .intel
            .logical_simulator
            .simulate_logic(Arc::clone(current_ref), ctx, sop_id, goal_text)
            .await;

        if let Ok(sim_res) = log_sim_result {
            if !sim_res.is_valid {
                let critical_flaws: Vec<_> = sim_res
                    .flaws
                    .iter()
                    .filter(|f| {
                        matches!(
                            f.severity,
                            sisyphus_intel::simulation::FlawSeverity::Critical
                        )
                    })
                    .collect();

                if !critical_flaws.is_empty() {
                    let flaw_msg = critical_flaws[0].description.clone();
                    pivot_reason = Some(format!("Logical Flaw Detected: {}", flaw_msg));
                }
            }
        }

        // ── 概率路径排序 ──────────────────────────────────
        // 使用概率规划器对路径排序并选择最优路径
        let ranked_path = self
            .probabilistic_planner
            .select_optimal_path(std::slice::from_ref(&prediction));
        if let Some(ref rp) = ranked_path {
            tracing::info!(
                sop_id,
                path_id = %rp.path_id,
                composite_score = rp.composite_score,
                risk = ?rp.risk_level,
                "Probabilistic planner: optimal path selected"
            );
            if let Some(ref tx) = stream_tx {
                let _ = tx.send(CognitiveEvent::CognitiveStep {
                    step: "Planning".into(),
                    message: format!(
                        "Optimal path: {} (score: {:.3}, risk: {:?})",
                        rp.path_id, rp.composite_score, rp.risk_level
                    ),
                    metrics: Some(CognitiveMetrics {
                        iteration,
                        deviation: 0.0,
                        confidence: rp.success_probability,
                        progress: (iteration as f32 / max_iterations as f32) * 100.0,
                        token_cost: 0,
                        stability: rp.confidence,
                        info_gain: 0.0,
                    }),
                });
            }
        }
        // ──────────────────────────────────────────────────────────────────

        // 推送预测事件
        if let Some(ref tx) = stream_tx {
            let _ = tx.send(CognitiveEvent::Predict {
                prediction: serde_json::to_value(&prediction).unwrap_or_default(),
            });
        }

        Ok((prediction, pivot_reason))
    }

    /// 阶段 2（执行）：运行 SOP 节点，在后台记录执行轨迹，
    /// 并产出本轮迭代结果。
    #[allow(clippy::too_many_arguments)]
    async fn execute_phase(
        &self,
        executor: &Executor,
        current_ref: &Arc<SOPGraph>,
        current_start: NodeIndex,
        exec_ctx: Option<Arc<ExecutionContext>>,
        prediction: &SimulationResult,
        sop_id: &str,
        goal_text: Option<&str>,
        iteration: usize,
        max_iterations: usize,
        convergence_threshold: f64,
        micro_reflector: Option<Arc<dyn sisyphus_types::MicroReflector>>,
        event_tx: &Option<mpsc::UnboundedSender<ExecutionEvent>>,
        broadcast_fn: &impl Fn(&str, ExecutionEvent),
        stream_tx: &Option<mpsc::UnboundedSender<CognitiveEvent>>,
        pivot_reason: &mut Option<String>,
    ) -> Result<(Arc<ExecutionContext>, NodeOutput), CoreError> {
        if let Some(mut status) = self.execution_manager.active_executions.get_mut(sop_id) {
            status.status = "Executing".into();
            status
                .thought_logs
                .push(format!("Iteration {}: Executing SOP nodes...", iteration));
            status.last_updated = iter_now();
        }
        broadcast_fn(
            sop_id,
            ExecutionEvent::CognitiveStep {
                step: "Executing".into(),
                message: format!("Iteration {}: Executing SOP nodes...", iteration),
                metrics: Some(CognitiveMetrics {
                    iteration,
                    deviation: 0.0,
                    confidence: prediction.success_probability,
                    progress: ((iteration as f32 + 0.5) / max_iterations as f32) * 100.0,
                    token_cost: 0,
                    stability: prediction.predicted_stability,
                    info_gain: 0.0,
                }),
            },
        );

        // ── 轨迹记录设置 ──────────────────────────────────────
        let (trace_tx, trace_rx) = mpsc::unbounded_channel::<ExecutionEvent>();
        spawn_trace_recorder(
            trace_rx,
            self.execution_manager.state_store.clone(),
            sop_id.to_string(),
            event_tx.clone(),
            self.anomaly_detector.clone(),
            Some(self.execution_manager.cognitive_trace.clone()),
        );

        // 2. 执行（Execute）
        tracing::info!(iteration, "phase=execute");
        let final_ctx = match executor
            .execute_with_context(
                Arc::clone(current_ref),
                current_start,
                exec_ctx,
                Some(trace_tx),
                self.execution_manager.tool_router.clone(),
                Some(Arc::new(SisyphusSupervisor::new(
                    self.intelligence_manager.intel.reflector.clone(),
                    prediction.clone(),
                    Arc::clone(current_ref),
                    convergence_threshold,
                    self.experience_bank.clone(),
                    goal_text.unwrap_or(sop_id).to_string(),
                ))),
                micro_reflector,
                self.visual_manager.clone(),
            )
            .instrument(tracing::info_span!("execute_with_context", iteration))
            .await
        {
            Ok(ctx) => ctx,
            Err(CoreError::StrategicPivot(reason)) => {
                *pivot_reason = Some(reason);
                Arc::new(ExecutionContext::new())
            }
            Err(e) => return Err(CoreError::Internal(e.to_string())),
        };

        let result = if let Some(reason) = pivot_reason.as_ref() {
            NodeOutput::from(Value::String(format!("Strategic Pivot: {}", reason)))
        } else {
            let start_name = current_ref
                .graph
                .node_weight(current_start)
                .map(|n| n.name().to_string())
                .ok_or_else(|| CoreError::Internal("Start node not found".into()))?;
            final_ctx
                .get_result(&start_name)
                .ok_or(CoreError::NoResultProduced)?
        };

        // 推送执行事件
        if let Some(ref tx) = stream_tx {
            let _ = tx.send(CognitiveEvent::Execute {
                node: current_start.index().to_string(),
                result: result.clone(),
            });
        }

        Ok((final_ctx, result))
    }

    /// 阶段 3（反思）：构建执行轨迹，运行反思与深度
    /// 反思，批判结果，并将洞察记录到经验库。
    #[allow(clippy::too_many_arguments)]
    async fn reflect_phase(
        &self,
        current_ref: &Arc<SOPGraph>,
        final_ctx: &Arc<ExecutionContext>,
        prediction: &SimulationResult,
        pivot_reason: &Option<String>,
        sop_id: &str,
        iteration: usize,
        max_iterations: usize,
        convergence_threshold: f64,
        goal_text: Option<&str>,
        stream_tx: &Option<mpsc::UnboundedSender<CognitiveEvent>>,
    ) -> Result<ReflectionReport, CoreError> {
        // 3. 反思（Reflect）
        tracing::info!(iteration, "phase=reflect");
        let mut trace = String::with_capacity(current_ref.graph.node_count() * 64);
        for node in current_ref.graph.node_indices() {
            if let Some(nw) = current_ref.graph.node_weight(node) {
                trace.push_str(nw.name());
                trace.push(':');
                if let Some(out) = final_ctx.get_result(nw.name()) {
                    trace.push_str(&out.to_string());
                }
                trace.push('\n');
            }
        }

        let mut reflection = if let Some(reason) = pivot_reason.as_ref() {
            sisyphus_intel::reflect::ReflectionReport {
                deviation_score: convergence_threshold + 0.1,
                suggested_improvements: vec![reason.clone()],
                original_sop_id: sop_id.to_string(),
                evolved_sop_structure: None,
                records: vec![],
            }
        } else {
            self.intelligence_manager
                .reflect_on_trace(&trace, prediction, final_ctx)
                .instrument(tracing::info_span!("reflect_on_trace", iteration))
                .await
                .map_err(|e| CoreError::Internal(e.to_string()))?
        };

        // ── 深度反思分析 ──────────────────────────────────────
        // 使用深度反思器进行多维偏差分析
        {
            let mut deep_reflector = self.deep_reflector.write();
            let deep_report = deep_reflector.analyze(&reflection, prediction);

            tracing::info!(
                sop_id,
                overall_score = deep_report.overall_score,
                corrections = deep_report.corrections.len(),
                patterns = deep_report.patterns.len(),
                "Deep reflection: {} dimensions analyzed, {} corrections suggested",
                deep_report.dimensions.len(),
                deep_report.corrections.len()
            );

            // 将高优先级修正作为额外建议应用
            for correction in &deep_report.corrections {
                if correction.priority <= sisyphus_intel::deep_reflector::CorrectionPriority::High {
                    reflection.suggested_improvements.push(format!(
                        "[DEEP {}] {}: {}",
                        format!("{:?}", correction.priority).to_uppercase(),
                        correction.target_node,
                        correction.description
                    ));
                }
            }

            // 从反思结果中记录失败模式
            if let Some(reason) = pivot_reason.as_ref() {
                let mut lib = self.failure_pattern_library.write();
                lib.record_failure(
                    reflection
                        .records
                        .first()
                        .map(|r| r.node.as_str())
                        .unwrap_or("unknown"),
                    sisyphus_intel::failure_pattern_evolver::FailureCategory::LogicError,
                    reason,
                );
            }

            // 推送深度反思事件
            if let Some(ref tx) = stream_tx {
                let _ = tx.send(CognitiveEvent::CognitiveStep {
                    step: "DeepReflect".into(),
                    message: format!(
                        "Score: {:.3} | {} corrections, {} patterns detected",
                        deep_report.overall_score,
                        deep_report.corrections.len(),
                        deep_report.patterns.len()
                    ),
                    metrics: Some(CognitiveMetrics {
                        iteration,
                        deviation: deep_report.overall_score,
                        confidence: 1.0 - deep_report.overall_score,
                        progress: (iteration as f32 / max_iterations as f32) * 100.0,
                        token_cost: 0,
                        stability: prediction.predicted_stability,
                        info_gain: 0.0,
                    }),
                });
            }
        }
        // ──────────────────────────────────────────────────────────────────

        // 推送反思事件
        if let Some(ref tx) = stream_tx {
            let _ = tx.send(CognitiveEvent::Reflect {
                report: serde_json::to_value(&reflection).unwrap_or_default(),
            });
        }

        // 批判（Critique）
        let critique = self
            .intelligence_manager
            .critique_reflection(&reflection, prediction)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        reflection
            .suggested_improvements
            .push(format!("[CRITIQUE] {}", critique.suggested_refinement));

        // 从反思中提炼语义洞察并记录到经验库
        let insight = if !reflection.suggested_improvements.is_empty() {
            Some(format!(
                "Deviation {:.4}. Key lessons: {}",
                reflection.deviation_score,
                reflection
                    .suggested_improvements
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ))
        } else {
            None
        };

        let exp_to_record = sisyphus_types::experience::ExperienceTuple {
            context_text: goal_text.unwrap_or(sop_id).to_string(),
            context_hash: format!("{:x}", md5::compute(goal_text.unwrap_or(sop_id))),
            action_id: sop_id.to_string(),
            outcome_success: reflection.deviation_score < 0.2,
            deviation_score: reflection.deviation_score,
            cost: 0,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            project_id: None,
            visual_embedding: None,
            embedding: None,
            insight,
        };
        let _ = self.experience_bank.record(exp_to_record).await;

        Ok(reflection)
    }

    /// 阶段 4（演进）：从记事本、经验库和失败模式中
    /// 收集演进洞察，然后演进 SOP 图谱。
    async fn evolve_phase(
        &self,
        ctx: &ExecutionContext,
        current_ref: &Arc<SOPGraph>,
        reflection: &ReflectionReport,
        prediction: &SimulationResult,
        sop_id: &str,
        goal_text: Option<&str>,
    ) -> Option<Arc<SOPGraph>> {
        // 5. 演进（Evolve）
        tracing::info!(sop_id, "phase=evolve");
        let mut evolution_insights = ctx
            .notepad
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .read_section("cognitive.insights")
            .cloned();

        // 用经验库增强演进洞察
        let exp_query = sisyphus_types::experience::ExperienceQuery {
            query_text: goal_text.unwrap_or(sop_id).to_string(),
            context_hash: format!("{:x}", md5::compute(goal_text.unwrap_or(sop_id))),
            action_id: None,
            project_id: None,
            visual_embedding: None,
            limit: 5,
        };

        if let Ok(exps) = self.experience_bank.query_similar(exp_query).await {
            let mut exp_hints = Vec::new();
            for exp in exps {
                if exp.outcome_success {
                    exp_hints.push(format!(
                        "Success Case [{}]: Goal: '{}', Deviation: {:.4}{}",
                        exp.action_id,
                        exp.context_text,
                        exp.deviation_score,
                        exp.insight
                            .as_ref()
                            .map(|i| format!(" | Insight: {}", i))
                            .unwrap_or_default()
                    ));
                } else {
                    exp_hints.push(format!(
                        "Failure Case [{}]: Goal: '{}', Deviation: {:.4}{}",
                        exp.action_id,
                        exp.context_text,
                        exp.deviation_score,
                        exp.insight
                            .as_ref()
                            .map(|i| format!(" | Insight: {}", i))
                            .unwrap_or_default()
                    ));
                }
            }
            if !exp_hints.is_empty() {
                let hints_str = format!("Semantic Historical Guidance:\n{}", exp_hints.join("\n"));
                evolution_insights = Some(match evolution_insights {
                    Some(existing) => format!("{}\n{}", existing, hints_str),
                    None => hints_str,
                });
            }
        }

        // ── 失败模式演进 ──────────────────────────────────────
        // 使用失败模式演进器生成针对性的演进操作，
        // 其依据是失败模式库中积累的失败模式。
        {
            let evolver =
                sisyphus_intel::FailurePatternEvolver::new(self.failure_pattern_library.clone());

            // 根据反思结果从失败模式生成演进操作
            let failure_ops = evolver.generate_evolution_ops(reflection);
            if !failure_ops.is_empty() {
                tracing::info!(
                    sop_id,
                    ops = failure_ops.len(),
                    "Failure pattern evolver generated {} evolution operations",
                    failure_ops.len()
                );

                // 将失败信息提示附加到演进洞察中
                let failure_hints: Vec<String> = failure_ops
                    .iter()
                    .map(|op| format!("Failure-Driven Evolution: {:?}", op))
                    .collect();

                evolution_insights = Some(match evolution_insights {
                    Some(existing) => format!(
                        "{}\nFailure-Driven Evolution:\n{}",
                        existing,
                        failure_hints.join("\n")
                    ),
                    None => format!("Failure-Driven Evolution:\n{}", failure_hints.join("\n")),
                });
            }
        }
        // ──────────────────────────────────────────────────────────────────

        self.intelligence_manager
            .intel
            .evolver
            .evolve(
                Arc::clone(current_ref),
                reflection,
                prediction,
                evolution_insights.as_deref(),
            )
            .await
            .ok()
    }

    /// 阶段 4.5（验证）：检查演进后的图谱是否真正提升了
    /// 成功概率或稳定性，再决定是否采纳。
    #[allow(clippy::too_many_arguments)]
    async fn verify_evolution(
        &self,
        evolved_graph: Arc<SOPGraph>,
        ctx: &ExecutionContext,
        prediction: &SimulationResult,
        sop_id: &str,
        project_id: Option<&str>,
        goal_text: Option<&str>,
        iteration: usize,
        stream_tx: &Option<mpsc::UnboundedSender<CognitiveEvent>>,
    ) -> Option<Arc<SOPGraph>> {
        // --- 验证步骤 ---
        // 验证演进后的图谱是否确实优于当前图谱。
        let mut final_evolved_graph = None;

        let verification_prediction = self
            .intelligence_manager
            .intel
            .simulator
            .simulate(
                Arc::clone(&evolved_graph),
                ctx,
                project_id.unwrap_or("default"),
                sop_id,
                goal_text,
            )
            .await
            .ok();

        if let Some(v_pred) = verification_prediction {
            // 仅当成功概率提升或稳定性显著改善时才接受演进结果。
            if v_pred.success_probability > prediction.success_probability
                || (v_pred.success_probability >= prediction.success_probability
                    && v_pred.predicted_stability > prediction.predicted_stability)
            {
                final_evolved_graph = Some(evolved_graph);

                // 记录验证成功
                if let Some(mut status) = self.execution_manager.active_executions.get_mut(sop_id) {
                    status.thought_logs.push(format!(
                        "Iteration {}: Evolution verified. Prob: {:.2} -> {:.2}",
                        iteration, prediction.success_probability, v_pred.success_probability
                    ));
                }
            } else {
                // 记录验证被拒
                if let Some(mut status) = self.execution_manager.active_executions.get_mut(sop_id) {
                    status.thought_logs.push(format!(
                        "Iteration {}: Evolution rejected. New Prob ({:.2}) <= Old Prob ({:.2})",
                        iteration, v_pred.success_probability, prediction.success_probability
                    ));
                }
            }
        } else {
            // 仿真失败，为安全起见视为拒绝
            if let Some(mut status) = self.execution_manager.active_executions.get_mut(sop_id) {
                status.thought_logs.push(format!(
                    "Iteration {}: Evolution verification failed (simulation error).",
                    iteration
                ));
            }
        }

        // 推送演进事件
        if let Some(ref tx) = stream_tx {
            let _ = tx.send(CognitiveEvent::Evolve {
                revised_plan: final_evolved_graph
                    .as_ref()
                    .and_then(|g| sisyphus_types::SOPGraphConfig::from_graph(g).ok())
                    .unwrap_or_default(),
            });
        }

        final_evolved_graph
    }
}

/// 启动后台任务，将执行轨迹事件记录到结构化
/// 存储，同时喂给异常检测器，并转发到用户事件流。
///
/// 微反思 (MicroReflection) 事件同时写入认知决策环形缓冲
/// （CognitiveStep 已由 broadcast_event 记录，此处只补充微反思）。
#[allow(clippy::too_many_arguments)]
fn spawn_trace_recorder(
    mut trace_rx: mpsc::UnboundedReceiver<ExecutionEvent>,
    store_clone: Arc<MfsStore>,
    sop_id_clone: String,
    user_event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    anomaly_det: Arc<AnomalyDetector>,
    cognitive_trace: Option<Arc<crate::execution_manager::CognitiveTraceBuffer>>,
) {
    tokio::spawn(async move {
        while let Some(event) = trace_rx.recv().await {
            // 0. 喂给 AnomalyDetector 用于 v2 运行时监控
            match &event {
                ExecutionEvent::NodeCompleted {
                    node_name,
                    fuel_consumed,
                    duration_ms,
                    ..
                } => {
                    anomaly_det.observe(
                        node_name,
                        *fuel_consumed as u64,
                        *duration_ms as u64,
                        true,
                    );
                }
                ExecutionEvent::NodeFailed {
                    node_name,
                    fuel_consumed,
                    duration_ms,
                    ..
                } => {
                    anomaly_det.observe(
                        node_name,
                        *fuel_consumed as u64,
                        *duration_ms as u64,
                        false,
                    );
                }
                _ => {}
            }
            // 1. 记录到结构化存储
            match &event {
                ExecutionEvent::NodeCompleted {
                    node_name,
                    output,
                    fuel_consumed,
                    duration_ms,
                } => {
                    let _ = store_clone
                        .record_trace_event(
                            &sop_id_clone,
                            node_name,
                            "ok",
                            Some(output.to_string()),
                            *fuel_consumed,
                            *duration_ms,
                        )
                        .await;
                }
                ExecutionEvent::NodeFailed {
                    node_name,
                    error,
                    fuel_consumed,
                    duration_ms,
                } => {
                    let _ = store_clone
                        .record_trace_event(
                            &sop_id_clone,
                            node_name,
                            "error",
                            Some(error.clone()),
                            *fuel_consumed,
                            *duration_ms,
                        )
                        .await;
                }
                _ => {}
            }
            // 2. 转发到原始事件流
            if let Some(ref buf) = cognitive_trace {
                if matches!(event, ExecutionEvent::MicroReflection { .. }) {
                    let timestamp_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    buf.push(crate::execution_manager::CognitiveTraceEntry {
                        timestamp_ms,
                        sop_id: sop_id_clone.clone(),
                        event: event.clone(),
                    });
                }
            }
            if let Some(ref tx) = user_event_tx {
                let _ = tx.send(event);
            }
        }
    });
}

/// 当前 Unix 时间戳（秒）。
fn iter_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
