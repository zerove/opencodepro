//! 内核执行：目标驱动的 SOP 执行与记录。
//!
//! `SisyphusKernel` 实现的一部分，为可读性从 `kernel.rs` 拆出。
//! 通过 `pub use kernel::*` 重新导出。

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::*;

impl SisyphusKernel {
    /// Execute goals.
    pub async fn execute_goals(
        &self,
        goals: &[&str],
        project_id: &str,
    ) -> Result<CompositeOutput, CoreError> {
        let ctx = ExecutionContext::new();

        let composite = sisyphus_intel::synthesize_composite(
            goals,
            &*self.intelligence_manager.synthesizer,
            &ctx,
            project_id,
        )
        .await
        .map_err(|e| CoreError::Internal(e.to_string()))?;

        self.execute_composite(&composite, None, project_id).await
    }

    /// 共同准备阶段：意图分析 → 经验检索 → 合成 → Swarm 审计 → 逻辑仿真门禁。
    /// 流式与非流式入口共享此管线（P0-2）。
    async fn prepare_goal_execution(
        &self,
        goal: &str,
        sop_id: &str,
        project_id: &str,
        ctx: &ExecutionContext,
    ) -> Result<(Arc<SOPGraph>, String), CoreError> {
        tracing::debug!("[execute_goal] Analyzing intent for goal: {goal}");

        let mode = IntentGate::analyze(goal, &self.intelligence_manager.llm_client).await;

        tracing::debug!("[execute_goal] Intent analyzed. Mode: {mode:?}");

        tracing::debug!("[execute_goal] Searching for successful experiences...");

        // 开始：经验检索（BEGIN_EXP_RETRIEVAL）

        let mut insights: Option<String> = None;

        // 结束：经验检索（END_EXP_RETRIEVAL）

        let context_hash = format!("{:x}", md5::compute(goal));

        let query = sisyphus_types::experience::ExperienceQuery {
            query_text: goal.to_string(),

            context_hash,

            action_id: None,

            project_id: None,
            visual_embedding: None,

            limit: 5,
        };

        if let Ok(exps) = self.experience_bank.query_similar(query).await {
            let best_exp = exps.into_iter().filter(|e| e.outcome_success).min_by(|a, b| {
                a.deviation_score
                    .partial_cmp(&b.deviation_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            if let Some(exp) = best_exp {
                if let Ok(graph) = self.load_sop(&exp.action_id, project_id).await {
                    insights = Some(format!(

                        "Found highly relevant successful experience (SOP: {}). Nodes: {}. Deviation: {:.4}", 

                        exp.action_id, graph.graph.node_indices().count(), exp.deviation_score

                    ));

                    tracing::debug!(
                        "[execute_goal] Found and injecting most relevant experience: {}",
                        exp.action_id
                    );
                }
            }
        }

        tracing::debug!("[execute_goal] Synthesizing SOP...");

        let synthesis = self
            .intelligence_manager
            .synthesize_goal(goal, ctx, project_id, mode, insights.as_deref())
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        tracing::debug!(
            "[execute_goal] Synthesis complete. Plan: {}",
            synthesis.plan_description
        );

        // --- Swarm 审计与逻辑仿真循环 ---
        tracing::debug!("[prepare] Entering refinement loop (Swarm Audit + Logical Simulation)...");
        let mut current_config = sisyphus_types::SOPGraphConfig::from_graph(&synthesis.graph)
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let mut simulation_attempts = 0;
        let max_simulation_attempts = 3;

        // P1-1：仿真检出的 Critical 缺陷回灌给下一轮 Swarm 审计，
        // 并在重试耗尽后阻断执行（此前为静默放行）。
        let mut flaw_feedback: Option<String> = None;

        loop {
            // 1. Swarm 审计用于高层优化
            let audit_goal = match flaw_feedback.as_deref() {
                Some(flaws) => format!(
                    "{goal}\n\nKnown critical flaws from logical simulation (must be fixed):\n{flaws}"
                ),
                None => goal.to_string(),
            };
            let evolved_config = self
                .swarm_coordinator
                .coordinate_evolution(&audit_goal, &current_config)
                .await
                .map_err(|e| CoreError::Internal(format!("Swarm audit failed: {}", e)))?;

            // 2. 逻辑仿真用于深层缺陷检测
            let sim_graph = sisyphus_types::SOPGraph::from_config(
                &evolved_config,
                self.execution_manager.tool_router.clone(),
                self.test_executor.clone(),
            )
            .map_err(|e| {
                CoreError::Internal(format!("Failed to rebuild graph for simulation: {}", e))
            })?;

            let sim_result = self
                .logical_simulator
                .simulate_logic(sim_graph, ctx, sop_id, Some(goal))
                .await
                .map_err(|e| CoreError::Internal(format!("Logical simulation failed: {}", e)))?;

            let critical_flaws: Vec<String> = sim_result
                .flaws
                .iter()
                .filter(|f| {
                    matches!(
                        f.severity,
                        sisyphus_intel::simulation::FlawSeverity::Critical
                    )
                })
                .map(|f| format!("[{}] {}", f.node_name, f.description))
                .collect();

            if sim_result.is_valid {
                tracing::debug!(
                    "[execute_goal] SOP passed logical simulation. Proceeding to execution."
                );
                current_config = evolved_config;
                break;
            }

            simulation_attempts += 1;
            if simulation_attempts >= max_simulation_attempts {
                if critical_flaws.is_empty() {
                    tracing::debug!("[execute_goal] Max simulation attempts reached with warnings only. Proceeding with best available.");
                    current_config = evolved_config;
                    break;
                }
                // P1-1 门禁：Critical 缺陷重试耗尽后阻断，不得静默执行。
                return Err(CoreError::Internal(format!(
                    "Logical simulation still reports {} critical flaw(s) after {max_simulation_attempts} refinement attempts; execution blocked: {}",
                    critical_flaws.len(),
                    critical_flaws.join("; ")
                )));
            }

            tracing::debug!(
                "[execute_goal] Simulation detected {critical_flaws_len} critical flaws. Triggering re-evolution...",
                critical_flaws_len = critical_flaws.len()
            );

            // 将缺陷描述注入下一轮审计目标，使架构师针对性修复。
            flaw_feedback = Some(critical_flaws.join("\n"));
            current_config = evolved_config;
        }

        let graph = sisyphus_types::SOPGraph::from_config(
            &current_config,
            self.execution_manager.tool_router.clone(),
            self.test_executor.clone(),
        )
        .map_err(|e| CoreError::Internal(format!("Final graph build failed: {}", e)))?;
        tracing::debug!("[prepare] Refinement loop complete. SOP ready for execution.");

        Ok((graph, synthesis.plan_description))
    }

    /// Execute goal.
    pub async fn execute_goal(
        &self,
        goal: &str,
        sop_id: &str,
        project_id: &str,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        context_json: Option<&str>,
    ) -> Result<(NodeOutput, String), CoreError> {
        let ctx = ExecutionContext::new();

        if let Some(ctx_str) = context_json {
            ctx.notepad
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .write_section("context".into(), ctx_str.into());
        }

        let (graph, plan_desc) =
            self.prepare_goal_execution(goal, sop_id, project_id, &ctx).await?;

        tracing::debug!("[execute_goal] Entering cognitive loop...");

        let (res, report_obj, _) = self
            .execute_with_cognition(graph, sop_id, Some(project_id), None, event_tx, None)
            .await?;

        tracing::debug!("[execute_goal] Cognitive loop completed.");

        let report_str = format!(
            "Plan: {} | Report: {} iterations, converged={}",
            plan_desc, report_obj.iterations, report_obj.converged
        );

        let mermaid_graph = report_obj.to_mermaid();

        Ok((
            res,
            format!(
                "{}\n\n### Execution Trace\n```mermaid\n{}```",
                report_str, mermaid_graph
            ),
        ))
    }

    /// Execute goal streaming.
    pub async fn execute_goal_streaming(
        &self,
        goal: &str,
        sop_id: &str,
        project_id: &str,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        stream_tx: Option<mpsc::UnboundedSender<sisyphus_types::CognitiveEvent>>,
    ) -> Result<(NodeOutput, String), CoreError> {
        let ctx = ExecutionContext::new();

        // P0-2：与非流式入口共享同一准备管线（意图 → 经验 → 合成 →
        // Swarm 审计 → 逻辑仿真门禁），仅事件交付方式不同。
        let (graph, plan_desc) =
            self.prepare_goal_execution(goal, sop_id, project_id, &ctx).await?;

        let (res, report_obj, _) = self
            .execute_with_cognition(graph, sop_id, Some(project_id), None, event_tx, stream_tx)
            .await?;

        let report_str = format!(
            "Plan: {} | Report: {} iterations, converged={}",
            plan_desc, report_obj.iterations, report_obj.converged
        );

        let mermaid_graph = report_obj.to_mermaid();

        Ok((
            res,
            format!(
                "{}\n\n### Execution Trace\n```mermaid\n{}```",
                report_str, mermaid_graph
            ),
        ))
    }

    /// Execute goal with project context.
    pub async fn execute_goal_with_project_context(
        &self,

        goal: &str,

        sop_id: &str,

        project_id: &str,

        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<(NodeOutput, String), CoreError> {
        let ctx_string =
            self.roadmap_manager.synthesize_project_context(project_id).await.map_err(|e| {
                CoreError::Internal(format!("Failed to synthesize project context: {}", e))
            })?;

        let augmented_goal = format!("{}\n\n---\nCurrent project context:\n{}", goal, ctx_string);

        self.execute_goal(&augmented_goal, sop_id, project_id, event_tx, None).await
    }

    /// Execute with project context, returning the structured cognitive
    /// report so callers can persist real metrics instead of parsing
    /// display strings (P2-2).
    pub async fn execute_goal_with_context_structured(
        &self,
        goal: &str,
        sop_id: &str,
        project_id: &str,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<
        (
            NodeOutput,
            String,
            crate::strategic_manager::CognitiveReport,
        ),
        CoreError,
    > {
        let ctx_string =
            self.roadmap_manager.synthesize_project_context(project_id).await.map_err(|e| {
                CoreError::Internal(format!("Failed to synthesize project context: {}", e))
            })?;

        let augmented_goal = format!("{}\n\n---\nCurrent project context:\n{}", goal, ctx_string);
        let ctx = ExecutionContext::new();

        let (graph, plan_desc) =
            self.prepare_goal_execution(&augmented_goal, sop_id, project_id, &ctx).await?;

        let (res, report_obj, _) = self
            .execute_with_cognition(graph, sop_id, Some(project_id), None, event_tx, None)
            .await?;

        let report_str = format!(
            "Plan: {} | Report: {} iterations, converged={}",
            plan_desc, report_obj.iterations, report_obj.converged
        );
        let report_str = format!(
            "{report_str}\n\n### Execution Trace\n```mermaid\n{}```",
            report_obj.to_mermaid()
        );

        Ok((res, report_str, report_obj))
    }

    #[allow(clippy::too_many_arguments)]
    /// Execute and record.
    pub async fn execute_and_record(
        &self,

        goal: &str,

        sop_id: &str,

        project_id: &str,

        milestone_id: &str,

        phase_id: &str,

        goal_id: &str,

        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<(NodeOutput, String, Option<sisyphus_roadmap::RoadmapHealth>), CoreError> {
        let start = std::time::Instant::now();

        // P2-2：改走结构化路径，用 CognitiveReport.final_deviation 记录
        // 真实偏差，取代对展示串的关键词猜测（旧逻辑恒为 None 或 0.5）。
        let result =
            self.execute_goal_with_context_structured(goal, sop_id, project_id, event_tx).await;

        let (success, deviation, error) = match &result {
            Ok((_output, _report_str, structured)) => {
                (true, Some(structured.final_deviation.clamp(0.0, 1.0)), None)
            },

            Err(e) => (false, None, Some(e.to_string())),
        };

        let elapsed = start.elapsed();

        // P2-1：timestamp 必须是墙钟时间；elapsed 只进 duration_secs。
        let wall_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let outcome = sisyphus_roadmap::ExecutionOutcome {
            goal_id: goal_id.to_string(),

            project_id: project_id.to_string(),

            milestone_id: milestone_id.to_string(),

            phase_id: phase_id.to_string(),

            success,

            deviation_score: deviation,

            error,

            timestamp: wall_now,

            actual_cost: 0.0, // actual cost tracking TBD at this layer

            duration_secs: elapsed.as_secs_f64(),
        };

        if let Err(e) = self.roadmap_manager.record_outcome(&outcome).await {
            tracing::warn!(
                project_id,
                goal_id,
                error = %e,
                "Failed to persist execution outcome; health evaluation will use stale data"
            );
        }

        // 记录结构化经验用于蒙特卡洛偏置
        let mut experience = sisyphus_types::experience::ExperienceTuple {
            context_text: goal.to_string(),
            context_hash: format!("{:x}", md5::compute(goal)),
            action_id: sop_id.to_string(),
            project_id: Some(project_id.to_string()),
            outcome_success: success,
            deviation_score: deviation.unwrap_or(0.0),
            cost: 0, // Actual cost tracking TBD
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            embedding: None,
            visual_embedding: None,
            insight: None,
            version: 0,
            vector_clock: std::collections::BTreeMap::new(),
        };

        // 成功后捕获视觉经验
        if success {
            if let Some(ref v_manager) = self.strategic_manager.visual_manager {
                // 在真实系统中，我们会捕获当前截图。
                // 目前我们用基于目标派生的确定性视觉状态来模拟，
                // 使不同目标映射到不同的状态。
                let dummy_image = sisyphus_visual::simulated_visual_state(goal);
                if let Ok(vis_emb) = v_manager.get_embedding(&dummy_image).await {
                    experience.visual_embedding = Some(vis_emb);
                }
            }
        }

        if let Err(e) = self.experience_bank.record(experience).await {
            tracing::warn!(project_id, error = %e, "Failed to record experience");
        }

        if !success {
            let health = self.roadmap_manager.apply_blocking(project_id).await.ok();

            return result.map(|(o, r, _structured)| (o, r, health));
        }

        let health = self.roadmap_manager.evaluate_health(project_id).await.ok();

        // 自动触发战略反思，分析路线图轨迹并提出转向建议。

        self.strategic_reflect(project_id).await;

        result.map(|(o, r, _structured)| (o, r, health))
    }

    /// Execute composite.
    pub async fn execute_composite(
        &self,
        composite: &CompositeGraph,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        project_id: &str,
    ) -> Result<CompositeOutput, CoreError> {
        let order = if composite.execution_order.is_empty() {
            DependencyResolver::resolve(composite)?
        } else {
            composite.execution_order.clone()
        };

        let order_len = order.len();
        let project_id_owned = project_id.to_string();

        let mut results: HashMap<String, (NodeOutput, String)> = HashMap::with_capacity(order_len);

        let accumulated: Arc<DashMap<String, NodeOutput>> =
            Arc::new(DashMap::with_capacity(order_len));

        #[allow(clippy::type_complexity)]
        let mut join_set: JoinSet<(
            String,
            Result<
                (
                    NodeOutput,
                    crate::strategic_manager::CognitiveReport,
                    Option<Arc<SOPGraph>>,
                ),
                CoreError,
            >,
        )> = JoinSet::new();

        let in_degree: Arc<DashMap<String, usize>> = Arc::new(DashMap::with_capacity(order_len));

        for sop_id in &order {
            in_degree.entry(sop_id.clone()).or_insert(0);
        }

        for dep in &composite.dependencies {
            *in_degree.entry(dep.target_sop_id.clone()).or_insert(0) += 1;
        }

        let mut graph_cache: HashMap<String, (Arc<SOPGraph>, NodeIndex)> =
            HashMap::with_capacity(order_len);

        for sop_id in &order {
            let config = composite.graphs.get(sop_id).ok_or_else(|| {
                CoreError::Internal(format!("Graph {} not found in composite", sop_id))
            })?;

            let graph = SOPGraph::from_config(
                config,
                self.execution_manager.tool_router.clone(),
                self.test_executor.clone(),
            )
            .map_err(|e| CoreError::Internal(e.to_string()))?;

            let start_node = graph
                .graph
                .node_indices()
                .next()
                .ok_or_else(|| CoreError::Internal(format!("Graph {} has no nodes", sop_id)))?;

            graph_cache.insert(sop_id.clone(), (graph, start_node));
        }

        let composite = Arc::new(composite.clone());

        let self_arc = Arc::new(SisyphusKernel {
            execution_manager: Arc::clone(&self.execution_manager),

            intelligence_manager: Arc::clone(&self.intelligence_manager),

            strategic_manager: Arc::clone(&self.strategic_manager),
            swarm_coordinator: Arc::clone(&self.swarm_coordinator),
            logical_simulator: Arc::clone(&self.logical_simulator),
            consensus: Arc::clone(&self.consensus),
            intervention_manager: Arc::clone(&self.intervention_manager),

            channel: Arc::clone(&self.channel),

            benchmark: Arc::clone(&self.benchmark),

            meta_learner: Arc::clone(&self.meta_learner),

            roadmap_manager: Arc::clone(&self.roadmap_manager),

            proposal_manager: Arc::clone(&self.proposal_manager),

            experience_bank: Arc::clone(&self.experience_bank),

            micro_reflector: self.micro_reflector.clone(),

            test_executor: self.test_executor.clone(),

            strategic_reflector: self.strategic_reflector.clone(),

            milestone_generator: self.milestone_generator.clone(),

            resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new(),
        });

        for sop_id in &order {
            if in_degree.get(sop_id).map(|d| *d).unwrap_or(0) == 0 {
                if let Some((graph, _start_node)) = graph_cache.remove(sop_id) {
                    let kernel = Arc::clone(&self_arc);

                    let acc = Arc::clone(&accumulated);

                    let composite_ref = Arc::clone(&composite);

                    let tx = event_tx.clone();

                    let sid = sop_id.clone();

                    let pid = project_id_owned.clone();
                    join_set.spawn(async move {
                        let dep_ctx = build_dep_ctx_for(&sid, &composite_ref, &acc);

                        let result = kernel
                            .execute_with_cognition(graph, &sid, Some(&pid), dep_ctx, tx, None)
                            .await;

                        (sid, result)
                    });
                }
            }
        }

        while let Some(res) = join_set.join_next().await {
            let (completed_id, outcome) = res.map_err(|e| CoreError::Internal(e.to_string()))?;

            let (node_output, report_obj, _evolved) = outcome?;

            let report = format!(
                "Converged: {}, Iterations: {}",
                report_obj.converged, report_obj.iterations
            );

            accumulated.insert(completed_id.clone(), node_output.clone());

            results.insert(completed_id.clone(), (node_output, report));

            let ready: Vec<String> = {
                let mut ready = Vec::new();

                for dep in &composite.dependencies {
                    if dep.source_sop_id == completed_id {
                        if let Some(mut d) = in_degree.get_mut(&dep.target_sop_id) {
                            *d -= 1;

                            if *d == 0 {
                                ready.push(dep.target_sop_id.clone());
                            }
                        }
                    }
                }

                ready
            };

            for sid in ready {
                if let Some((graph, _start_node)) = graph_cache.remove(&sid) {
                    let kernel = Arc::clone(&self_arc);

                    let acc = Arc::clone(&accumulated);

                    let composite_ref = Arc::clone(&composite);

                    let tx = event_tx.clone();

                    let pid = project_id_owned.clone();
                    join_set.spawn(async move {
                        let dep_ctx = build_dep_ctx_for(&sid, &composite_ref, &acc);

                        let result = kernel
                            .execute_with_cognition(graph, &sid, Some(&pid), dep_ctx, tx, None)
                            .await;

                        (sid, result)
                    });
                }
            }
        }

        Ok(CompositeOutput {
            results,

            execution_order: order,
        })
    }
}
