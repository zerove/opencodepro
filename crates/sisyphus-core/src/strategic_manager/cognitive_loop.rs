//! 认知循环执行：预测（predict）→ 执行（execute）→ 反思（reflect）→ 演进（evolve）的循环。
//!
//! [`run_cognitive_loop`](StrategicManager::run_cognitive_loop) 编排整个循环；
//! 每个阶段被拆分为独立的辅助函数，使编排逻辑保持清晰，
//! 且每个阶段都可以独立演进。

use parking_lot::Mutex;
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

use crate::cognitive_loop_v2::{AdaptiveDepthController, AnomalyDetector};
use crate::{CoreError, ExecutionContext, ExecutionEvent, ExecutionStatus, Executor, NodeOutput};

// OpenTelemetry span helpers
use sisyphus_metrics::observability::{cognitive_loop_span, evolution_span, reflection_span};

use super::{CognitiveIteration, CognitiveReport, StrategicManager};

impl StrategicManager {
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip_all, fields(sop_id = %sop_id, graph_nodes = sop_graph.graph.node_count()))]
    /// Run the full cognitive loop: predict → execute → reflect → evolve.
    ///
    /// Iterates the SOP graph until all nodes complete or a stop condition fires.
    /// Each iteration uses the LLM-driven predictor to choose the next node,
    /// executes it via the tool registry, reflects on the outcome through the
    /// configured [`MicroReflector`], and applies any graph evolution produced
    /// by the evolver before the next iteration.
    ///
    /// # Arguments
    /// * `sop_graph` - The cognitive DAG to execute (shared, may be cloned)
    /// * `sop_id` - Identifier of the SOP definition being run
    /// * `goal_text` - Optional user-facing goal that seeded this SOP
    /// * `project_id` - Optional project scope for outcome persistence
    /// * `initial_ctx` - Optional pre-populated execution context
    /// * `event_tx` - Optional channel for [`ExecutionEvent`]s (for TUI/SSE)
    /// * `broadcast_fn` - Synchronous broadcast hook (e.g. metrics emission)
    /// * `stream_tx` - Optional channel for [`CognitiveEvent`] streaming
    /// * `micro_reflector` - Optional custom reflection strategy
    ///
    /// # Returns
    /// The final node output, the cognitive report (reflections + evolution),
    /// and an optional updated graph if the evolver produced structural changes.
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

        // P0-1：每次运行使用独立的控制器与检测器实例（从共享配置模板
        // 继承参数），不再触碰 StrategicManager 上的共享实例。并发运行
        // 的置信度历史/异常基线互不污染，也无需在启动时 reset（reset
        // 会清掉其他并发运行的状态）。
        let depth_controller = {
            let shared = &self.depth_controller;
            let local = AdaptiveDepthController::new(
                *shared.target_confidence.read(),
                *shared.min_improvement.read(),
                shared.max_iterations,
                shared.min_iterations,
            );
            local
        };
        let anomaly_detector = {
            let shared = self.anomaly_detector.lock();
            Arc::new(Mutex::new(AnomalyDetector::new(
                shared.fuel_threshold,
                shared.duration_threshold,
                shared.error_rate_threshold,
            )))
        };

        // A2 修复：以 sop_id + 纳秒时间戳为 active_executions key，
        // 避免同一 sop 的并发执行互相覆盖状态。
        // 状态展示仍使用 sop_id 便于用户识别，phase 函数接收 sop_id 做广播。
        let active_key = format!(
            "{sop_id}@{nanos}",
            sop_id = sop_id,
            nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );

        // --- 热启动阶段（HOT START） ---
        let (mut current_ref, hot_start_active) =
            self.hot_start_phase(&sop_graph, goal_text, project_id, &stream_tx).await;

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
            // --- 动态深度调整（P1-4 去重：单一 ComplexityAnalyzer 分析）---
            // 此前这里与 AdaptiveDepthController::initial_budget 内部对同一输入
            // 各做一次 ComplexityAnalyzer::analyze，调参面翻倍；现在只分析一次，
            // 控制器预算经 budget_from_depth 复用本处结果。
            let depth = sisyphus_intel::ComplexityAnalyzer::analyze(
                self.experience_bank.clone(),
                &current_ref,
                goal_text.unwrap_or(sop_id),
            )
            .await;

            let (mut d_max, d_thresh) = depth.to_config();

            // v2：控制器初始预算，复用上方分析结果（热启动减半前计算，
            // 与原 initial_budget 行为等价——其内部分析同样不含热启动逻辑）
            let v2_budget = depth_controller.budget_from_depth(d_max) as u32;

            // 如果热启动生效，可以大幅减少迭代次数
            if hot_start_active {
                d_max = (d_max / 2).max(2);
            }

            // 用户显式配置的 max_iterations 是硬上限（评审 A4 遗留缺口修复）：
            // 自适应深度只能在预算内减少迭代，不得突破用户设置的上限。
            let final_max =
                effective_max_iterations(d_max, v2_budget, self.config.read().max_iterations);

            // 基线仅用于日志对比；动态结果不写回共享 config（见 dbc2f7a）。
            // 此前的写回-读回模式会让并发执行的循环互相覆盖 max_iterations/convergence_threshold
            // （写回值只有本运行读一次，其余全是污染）。行为与原先完全等价：
            // 本轮实际生效的就是下方局部返回的 final_max/eff_threshold。
            // 完整背景见 docs/work-summaries/2026-08-26_architecture-review.md P0-1。
            let baseline = self.config.read();
            let user_threshold = baseline.convergence_threshold;
            // 自适应阈值只能在用户包络内收紧（见 effective_convergence_threshold）。
            // 接线修复：此前 d_thresh 直接生效，cognitive_loop.convergence_threshold
            // 只被 apply_cognitive_loop_config 写进日志，从不影响循环行为。
            let eff_threshold = effective_convergence_threshold(d_thresh, user_threshold);
            tracing::info!(
                "Loop parameters: baseline max_iterations {}, effective {} (v2 budget {}), \
                 threshold adaptive {} -> effective {} (user {})",
                baseline.max_iterations,
                final_max,
                v2_budget,
                d_thresh,
                eff_threshold,
                user_threshold
            );

            (final_max as usize, eff_threshold)
        };

        let span = tracing::info_span!("strategic.run_cognitive_loop", sop_id = %sop_id, graph_nodes = sop_graph.graph.node_count());
        let _enter = span.enter();

        let now =
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
        self.execution_manager.active_executions.insert(
            active_key.clone(),
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
        // 统计「本轮产出的是真实执行结果」而非 StrategicPivot 占位文本的次数。
        //
        // 背景：`execute_phase` 捕获 `CoreError::StrategicPivot` 后不向上报错，
        // 而是构造 `"Strategic Pivot: ..."` 文本当作节点输出（见 execute_phase
        // 内的 result 分支）。若不做区分，一轮里所有节点都失败的情况会被
        // 伪装成正常执行，最终 `run_cognitive_loop` 无条件 `Ok`，桥接层再
        // 硬编码 status="SUCCESS"，调用方将无法分辨「执行成功」与「一执行就崩」。
        //
        // 语义边界：**只有当全部迭代都只产出 pivot 占位文本、零真实执行结果时**
        // 才判定为失败；只要有任意一轮拿到了真实执行结果，就仍然视为成功
        // （中途 pivot 后恢复是合法路径）。
        //
        // 关键：统计依据是 `exec_pivot`（执行期 pivot 信号）而**不是**
        // `pivot_reason`。后者有两个来源——预测阶段的「Logical Flaw Detected」
        // 与执行阶段的 StrategicPivot——前者并不代表本轮没执行。
        let mut real_result_iterations: usize = 0;

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
            // `exec_pivot` 只标记**执行阶段**发生的 StrategicPivot，与
            // `pivot_reason` 的预测期赋值（Logical Flaw）严格区分。
            let mut exec_pivot = false;
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
                    &anomaly_detector,
                    &mut pivot_reason,
                    &mut exec_pivot,
                )
                .instrument(exec_span)
                .await?;

            final_result = result.clone();
            if !exec_pivot {
                real_result_iterations += 1;
            }

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
                    project_id,
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
                    project_id,
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
            let should_continue = depth_controller.observe_iteration(confidence);

            // v2：检查异常报告是否触发额外反思
            let anomaly_report = anomaly_detector.lock().generate_report();
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
            self.execution_manager.cognitive_maps.insert(sop_id.to_string(), cog_map);

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

        let converged =
            iterations.last().map(|i| i.deviation_score < convergence_threshold).unwrap_or(false);

        // 零真实产出的硬失败：每一轮都以 StrategicPivot 收场。
        //
        // 若不加拦截，`final_result` 会是 `"Strategic Pivot: ..."` 占位文本，
        // 上层（run_execution → execute_sop_impl）会把它当作正常结果，
        // 最终向客户端返回 status="SUCCESS"，把「所有节点都执行失败」
        // 伪装成成功。有了这个分支，客户端会收到真正的错误。
        //
        // 判定条件用「零真实结果」而非「converged」：中途 pivot 后恢复是
        // 合法路径，不应被误判为失败。
        if real_result_iterations == 0 {
            // `final_result` 此时就是占位文本，取其字符串作为 pivot 原因。
            let last_pivot = final_result.as_value().as_str().unwrap_or("unknown").to_string();
            return Err(CoreError::AllIterationsPivoted(
                iterations.len(),
                last_pivot,
            ));
        }

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
        // A2 修复：使用基于 sop_id + 启动时间的唯一 key 查询 active_executions。
        let active_key = sop_id.to_string();

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
            if let Some(mut status) = self.execution_manager.active_executions.get_mut(&active_key)
            {
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
        if let Some(mut status) = self.execution_manager.active_executions.get_mut(&active_key) {
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
        let ranked_path =
            self.probabilistic_planner.select_optimal_path(std::slice::from_ref(&prediction));
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
        anomaly_detector: &Arc<Mutex<AnomalyDetector>>,
        pivot_reason: &mut Option<String>,
        exec_pivot: &mut bool,
    ) -> Result<(Arc<ExecutionContext>, NodeOutput), CoreError> {
        // A2 修复：使用基于 sop_id + 启动时间的唯一 key 查询 active_executions。
        let active_key = sop_id.to_string();

        if let Some(mut status) = self.execution_manager.active_executions.get_mut(&active_key) {
            status.status = "Executing".into();
            status.thought_logs.push(format!("Iteration {}: Executing SOP nodes...", iteration));
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
            anomaly_detector.clone(),
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
                *exec_pivot = true;
                Arc::new(ExecutionContext::new())
            },
            Err(e) => return Err(e),
        };

        // 占位文本必须以**执行期** pivot 为准，不能看 `pivot_reason`：
        // 后者会被预测阶段的「Logical Flaw Detected」污染（第 723 行），
        // 导致一次执行成功的迭代被替换成 `"Strategic Pivot: ..."` 假结果，
        // 而上层仍会把它当正常结果返回给客户端。
        let result = if *exec_pivot {
            NodeOutput::from(Value::String(format!(
                "Strategic Pivot: {}",
                pivot_reason.as_deref().unwrap_or("unknown")
            )))
        } else {
            let start_name = current_ref
                .graph
                .node_weight(current_start)
                .map(|n| n.name().to_string())
                .ok_or_else(|| CoreError::Internal("Start node not found".into()))?;
            final_ctx.get_result(&start_name).ok_or(CoreError::NoResultProduced)?
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
        project_id: Option<&str>,
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
                // pivot 原因是给人看的信息，不是机器可执行的演进信号
                improvement_signals: vec![],
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
                    reflection.records.first().map(|r| r.node.as_str()).unwrap_or("unknown"),
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

        // Token 预算检查（review P2-8 step 2 接线）：
        // 若 budget 已耗尽到保留阈值以下，跳过 critique LLM 调用，
        // 改用宽松默认值（与 timeout_secs=0 语义一致），保留认知循环继续推进。
        if self.token_budget.read().should_skip_llm() {
            tracing::info!(sop_id, "Token budget exhausted; skipping critique LLM call");
            reflection
                .suggested_improvements
                .push("[CRITIQUE] skipped: token budget exhausted".to_string());
        } else {
            // 批判（Critique）。先读出统一的 LLM 阶段超时并立刻释放读锁，
            // 避免 parking_lot 守卫跨越 await 导致 future 不满足 Send。
            let llm_stage_timeout_secs = self.config.read().llm_stage_timeout_secs;
            let critique = self
                .intelligence_manager
                .critique_reflection(&reflection, prediction, llm_stage_timeout_secs)
                .await
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            reflection
                .suggested_improvements
                .push(format!("[CRITIQUE] {}", critique.suggested_refinement));
        }

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
            // 反思产生的经验必须落在项目作用域内，否则按项目检索时取不到，
            // 且多项目共享经验库时无法区分归属。
            project_id: project_id.map(str::to_string),
            visual_embedding: None,
            embedding: None,
            insight,
            version: 0,
            vector_clock: std::collections::BTreeMap::new(),
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
        project_id: Option<&str>,
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
            // 演进只应借鉴同项目的经验，跨项目取用会让一个项目的失败模式
            // 污染另一个项目的演进方向。
            project_id: project_id.map(str::to_string),
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
        // A2 修复：使用基于 sop_id + 启动时间的唯一 key 查询 active_executions。
        let active_key = sop_id.to_string();

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
                if let Some(mut status) =
                    self.execution_manager.active_executions.get_mut(&active_key)
                {
                    status.thought_logs.push(format!(
                        "Iteration {}: Evolution verified. Prob: {:.2} -> {:.2}",
                        iteration, prediction.success_probability, v_pred.success_probability
                    ));
                }
            } else {
                // 记录验证被拒
                if let Some(mut status) =
                    self.execution_manager.active_executions.get_mut(&active_key)
                {
                    status.thought_logs.push(format!(
                        "Iteration {}: Evolution rejected. New Prob ({:.2}) <= Old Prob ({:.2})",
                        iteration, v_pred.success_probability, prediction.success_probability
                    ));
                }
            }
        } else {
            // 仿真失败，为安全起见视为拒绝
            if let Some(mut status) = self.execution_manager.active_executions.get_mut(&active_key)
            {
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
    anomaly_det: Arc<Mutex<AnomalyDetector>>,
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
                    anomaly_det.lock().observe(
                        node_name,
                        *fuel_consumed as u64,
                        *duration_ms as u64,
                        true,
                    );
                },
                ExecutionEvent::NodeFailed {
                    node_name,
                    fuel_consumed,
                    duration_ms,
                    ..
                } => {
                    anomaly_det.lock().observe(
                        node_name,
                        *fuel_consumed as u64,
                        *duration_ms as u64,
                        false,
                    );
                },
                _ => {},
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
                },
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
                },
                _ => {},
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
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// 计算认知循环的有效最大迭代次数（评审 A4 遗留缺口修复，275a435）。
///
/// 语义：自适应深度分析只能在用户配置预算内**减少**迭代，
/// 不得突破 `max_iterations` 硬上限。三个输入按优先级取最小值：
/// 1. `adaptive_depth`：复杂度分析得出的动态深度上限；
/// 2. `v2_budget`：深度控制器基于经验库给出的初始预算；
/// 3. `user_cap`：用户显式配置的 `max_iterations`（硬顶，最高优先级约束）。
pub fn effective_max_iterations(adaptive_depth: u32, v2_budget: u32, user_cap: u32) -> u32 {
    adaptive_depth.min(v2_budget).min(user_cap)
}

/// 计算认知循环的有效收敛阈值（`convergence_threshold` 接线修复）。
///
/// **背景**：ADR 005 决定用自适应深度控制器取代 v1 的静态
/// `convergence_threshold`，因此循环的实际偏差阈值改为由
/// [`sisyphus_intel::ComplexityAnalyzer`] 按复杂度分档给出
/// （Low 0.2 / Medium 0.05 / High 0.01 / Ultra 0.005，
/// 见 `sisyphus-intel/src/complexity.rs:17-24`）。
/// 但顶层 `cognitive_loop.convergence_threshold` 字段并未随之下线，
/// 而是被文档、配置模板和环境变量当作可用旋钮承诺：
/// - `config.example.toml:193-197`（「偏差低于此值时提前停止循环」）
/// - `book/src/performance.md:89`、`:99`
/// - `book/src/configuration.md:12,130,269`
/// - `SISYPHUS_COGNITIVE_LOOP_CONVERGENCE_THRESHOLD`
///
/// 结果：用户设置的值**从不生效**，而 `apply_cognitive_loop_config`
/// 只把它写进日志，看起来像是已应用。
///
/// **语义**：与 [`effective_max_iterations`] 同构——自适应分析只能在用户
/// 配置的包络内**收紧**，不得突破用户的明确设定：
/// 1. 取 `min(adaptive_threshold, user_threshold)`；
/// 2. 用户配置为 `0.0` 时视为「不额外收紧」，完全采用自适应档位。
///
/// 采用 min 而非直接采用用户值，是因为偏差阈值**越小越严格**
/// （偏差低于该值才算收敛），直接采用用户值会**放大**宽松度——
/// 用户写 0.15 时实际生效的 Ultra 档 0.005 比 0.15 严格得多，
/// 反向采用会把「更严」误当成「更松」。min 语义保证：
/// **用户在任一方向上设置的约束都不会被自适应分析突破**。
pub fn effective_convergence_threshold(adaptive_threshold: f64, user_threshold: f64) -> f64 {
    if user_threshold <= 0.0 {
        adaptive_threshold
    } else {
        adaptive_threshold.min(user_threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // rustc 1.98 unused_imports: Tbt/TokenBudgetConfig 在该文件测试中未直接使用
    //（之前由 cfg(test) 模块直接引用，但实际只在其他位置用 String 解析）

    #[test]
    fn test_effective_max_respects_user_cap() {
        // 用户上限低于自适应建议：硬顶必须生效。
        assert_eq!(effective_max_iterations(30, 30, 5), 5);
        assert_eq!(effective_max_iterations(20, 30, 10), 10);
    }

    #[test]
    fn test_effective_max_adaptive_can_only_reduce() {
        // 自适应/预算只能减少、不能放大超过用户上限。
        assert_eq!(effective_max_iterations(3, 30, 20), 3);
        assert_eq!(effective_max_iterations(30, 7, 20), 7);
        // 全部宽松时等于用户上限。
        assert_eq!(effective_max_iterations(u32::MAX, u32::MAX, 25), 25);
    }

    #[test]
    fn test_effective_max_zero_cap_disables_loop() {
        // 上限为 0 是显式的"禁止迭代"语义。
        assert_eq!(effective_max_iterations(30, 30, 0), 0);
    }

    /// 用户阈值比自适应档位更严时必须生效（min 语义的收紧方向）。
    #[test]
    fn test_effective_threshold_user_can_tighten() {
        // Ultra 档 0.005，用户要求 0.001 → 更严，采用用户值。
        assert!((effective_convergence_threshold(0.005, 0.001) - 0.001).abs() < f64::EPSILON);
        // Low 档 0.2，用户要求 0.15（文档默认）→ 采用用户值。
        assert!((effective_convergence_threshold(0.2, 0.15) - 0.15).abs() < f64::EPSILON);
    }

    /// 自适应档位比用户阈值更严时不得被放宽。
    ///
    /// 这是最容易写反的方向：用户写 0.15 但实际命中 Ultra 档 0.005，
    /// 若直接采用用户值就会把严格判定**放宽** 30 倍。
    #[test]
    fn test_effective_threshold_adaptive_is_never_loosened() {
        assert!((effective_convergence_threshold(0.005, 0.15) - 0.005).abs() < f64::EPSILON);
        assert!((effective_convergence_threshold(0.01, 0.15) - 0.01).abs() < f64::EPSILON);
        // 等值时无争议。
        assert!((effective_convergence_threshold(0.05, 0.05) - 0.05).abs() < f64::EPSILON);
    }

    /// 用户阈值 0.0 表示「不额外收紧」，完全采用自适应档位。
    #[test]
    fn test_effective_threshold_zero_means_no_override() {
        for adaptive in [0.2, 0.05, 0.01, 0.005] {
            assert!(
                (effective_convergence_threshold(adaptive, 0.0) - adaptive).abs() < f64::EPSILON
            );
        }
    }

    /// 四档复杂度全覆盖：用户包络永远不破。
    #[test]
    fn test_effective_threshold_covers_all_complexity_tiers() {
        // (ComplexityAnalyzer 档位, 对应阈值)
        for adaptive in [0.2, 0.05, 0.01, 0.005] {
            for user in [0.0, 0.001, 0.05, 0.15, 1.0] {
                let eff = effective_convergence_threshold(adaptive, user);
                assert!(
                    eff > 0.0,
                    "有效阈值必须为正：adaptive={adaptive} user={user}"
                );
                if user > 0.0 {
                    assert!(
                        eff <= adaptive + f64::EPSILON && eff <= user + f64::EPSILON,
                        "有效阈值必须同时不宽于自适应与用户设定：adaptive={adaptive} user={user} eff={eff}"
                    );
                } else {
                    assert!((eff - adaptive).abs() < f64::EPSILON);
                }
            }
        }
    }

    /// 验证 A2 修复：active_key 格式包含 sop_id + 唯一后缀，
    /// 同一 sop 多次启动时生成不同 key，避免 active_executions 互写。
    #[test]
    fn test_a2_active_key_uniqueness_per_invocation() {
        use std::time::SystemTime;
        let sop_id = "test_sop";
        // 模拟连续两次启动；实际唯一性来自纳秒级时间戳
        let key1 = format!(
            "{sop_id}@{nanos}",
            sop_id = sop_id,
            nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
        let key2 = format!(
            "{sop_id}@{nanos}",
            sop_id = sop_id,
            nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        assert!(key1.starts_with("test_sop@"));
        assert!(key2.starts_with("test_sop@"));
        // 两次启动产生不同的 key（纳秒级时间戳保证）
        assert_ne!(key1, key2, "并发启动必须产生不同 active_key");
    }
}
