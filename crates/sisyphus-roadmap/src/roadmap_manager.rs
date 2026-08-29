use crate::milestone_generator::MilestoneGenerator;
use crate::resource_optimizer::ResourceOptimizer;
use crate::strategic_reflection::{ProposalAction, StrategicReflection, StrategicReflector};
use crate::{
    Bottleneck, CostProjection, GoalResourceEstimate, ParallelizationOpportunity, RoadmapError,
    RoadmapHealth,
};
use anyhow::Result;
use sisyphus_mem::{roadmap_store::RoadmapSqlStore, MfsStore};
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase, Project, Roadmap};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// RoadmapManager（路线图管理器）
// ---------------------------------------------------------------------------
// RoadmapManager（路线图管理器）
// ---------------------------------------------------------------------------

/// 战略规划管理器，提供对 项目 → 里程碑 → 阶段 → 目标
/// 层级结构的 CRUD 操作，通过 MfsStore 持久化。
pub struct RoadmapManager {
    pub(crate) store: Arc<MfsStore>,
    pub(crate) sql_store: Arc<RoadmapSqlStore>,
    pub(crate) proposal_manager: Arc<crate::proposal_manager::ProposalManager>,
}

impl RoadmapManager {
    /// 创建由给定 `MfsStore` 与 `RoadmapSqlStore` 支撑的 `RoadmapManager`。
    pub fn new(
        store: Arc<MfsStore>,
        sql_store: Arc<RoadmapSqlStore>,
        proposal_manager: Arc<crate::proposal_manager::ProposalManager>,
    ) -> Self {
        Self {
            store,
            sql_store,
            proposal_manager,
        }
    }

    /// 获取提案管理器的引用。
    pub fn proposal_manager(&self) -> &Arc<crate::proposal_manager::ProposalManager> {
        &self.proposal_manager
    }

    // ── 项目 CRUD ────────────────────────────────────────────────

    /// 创建新项目并持久化。
    pub async fn create_project(
        &self,
        id: &str,
        name: &str,
        description: &str,
    ) -> Result<Project, RoadmapError> {
        let project = Project::new(id.to_string(), name.to_string(), description.to_string());

        self.sql_store.insert_project(&project).await.map_err(|e| {
            if e.to_string()
                .contains("UNIQUE constraint failed: projects.id")
            {
                RoadmapError::DuplicateProject(id.to_string())
            } else {
                RoadmapError::Internal(e.to_string())
            }
        })?;

        Ok(project)
    }

    /// 按 ID 检索项目并从细粒度存储中水合（hydrate）。
    pub async fn get_project(&self, id: &str) -> Result<Option<Project>, RoadmapError> {
        self.sql_store
            .get_project(id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// 更新已有项目（替换完整文档）。
    pub async fn update_project(&self, project: &Project) -> Result<(), RoadmapError> {
        self.sql_store
            .insert_project(project)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// 按 ID 删除项目。
    pub async fn delete_project(&self, id: &str) -> Result<(), RoadmapError> {
        self.sql_store
            .delete_project(id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))?;
        Ok(())
    }

    /// 列出全部项目 ID。
    pub async fn list_project_ids(&self) -> Result<Vec<String>, RoadmapError> {
        self.sql_store
            .list_projects()
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// 列出全部完整项目。
    pub async fn list_projects(&self) -> Result<Vec<Project>, RoadmapError> {
        let ids = self.list_project_ids().await?;
        let mut projects = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(project) = self.get_project(&id).await? {
                projects.push(project);
            }
        }
        Ok(projects)
    }

    // ── 里程碑操作 ────────────────────────────────────────

    /// 向项目添加里程碑。
    pub async fn add_milestone(
        &self,
        project_id: &str,
        milestone: Milestone,
    ) -> Result<(), RoadmapError> {
        self.sql_store
            .insert_milestone(
                &milestone.id,
                project_id,
                &milestone.name,
                &milestone.status.to_string(),
            )
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))?;

        for phase in &milestone.phases {
            let deps = phase.depends_on.join(",");
            self.sql_store
                .insert_phase(
                    project_id,
                    &phase.id,
                    &milestone.id,
                    &phase.name,
                    Some(deps),
                )
                .await
                .map_err(|e| RoadmapError::Internal(e.to_string()))?;

            for goal in &phase.goals {
                self.sql_store
                    .insert_goal(
                        project_id,
                        &goal.goal_id,
                        &phase.id,
                        &goal.description,
                        &goal.status.to_string(),
                    )
                    .await
                    .map_err(|e| RoadmapError::Internal(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// 更新项目内里程碑的状态。
    pub async fn update_milestone_status(
        &self,
        project_id: &str,
        milestone_id: &str,
        status: MilestoneStatus,
    ) -> Result<(), RoadmapError> {
        self.sql_store
            .update_milestone_status(project_id, milestone_id, &status.to_string())
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// 从项目移除里程碑。
    pub async fn remove_milestone(
        &self,
        project_id: &str,
        milestone_id: &str,
    ) -> Result<(), RoadmapError> {
        if self
            .sql_store
            .get_project(project_id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))?
            .is_none()
        {
            return Err(RoadmapError::ProjectNotFound(project_id.to_string()));
        }

        self.sql_store
            .delete_milestone(project_id, milestone_id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))?;

        Ok(())
    }

    // ── 阶段操作 ────────────────────────────────────────────

    /// 向项目内里程碑添加阶段。
    pub async fn add_phase(
        &self,
        project_id: &str,
        milestone_id: &str,
        phase: Phase,
    ) -> Result<(), RoadmapError> {
        let deps = phase.depends_on.join(",");
        self.sql_store
            .insert_phase(project_id, &phase.id, milestone_id, &phase.name, Some(deps))
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))?;

        for goal in &phase.goals {
            self.sql_store
                .insert_goal(
                    project_id,
                    &goal.goal_id,
                    &phase.id,
                    &goal.description,
                    &goal.status.to_string(),
                )
                .await
                .map_err(|e| RoadmapError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    /// 基于失败簇分析提交战略转移提案。
    pub async fn submit_strategic_shift(
        &self,
        project_id: &str,
        common_failure_mode: &str,
        evidence_count: usize,
        suggested_shift: &str,
    ) -> Result<(), RoadmapError> {
        let proposal_id = format!(
            "shift-{}-{}",
            project_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );

        let proposal = crate::strategic_reflection::StrategicProposal {
            proposal_id,
            project_id: project_id.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            trigger_reflection: crate::strategic_reflection::StrategicReflection {
                project_id: project_id.to_string(),
                project_name: "Cluster Failure Detected".to_string(),
                health_assessment: format!(
                    "Systematic failure detected in {} similar tasks.",
                    evidence_count
                ),
                trajectory: "Stagnating".to_string(),
                risk_factors: vec![common_failure_mode.to_string()],
                recommendations: vec![crate::strategic_reflection::StrategicRecommendation {
                    action: suggested_shift.to_string(),
                    rationale: format!(
                        "Cluster of {} failures showing pattern: {}",
                        evidence_count, common_failure_mode
                    ),
                    priority: "high".to_string(),
                    target_milestone: None,
                }],
                confidence: 0.9,
            },
            action: crate::strategic_reflection::ProposalAction::PivotTrajectory {
                new_trajectory: suggested_shift.to_string(),
                rationale: format!("Systematic failure mode: {}", common_failure_mode),
            },
            expected_impact: "Resolution of systemic failure cluster".to_string(),
            status: crate::strategic_reflection::ProposalStatus::Pending,
            reviewer: None,
        };

        self.proposal_manager
            .submit_proposal(project_id, proposal)
            .await
    }

    /// 向里程碑内的阶段添加目标。
    pub async fn add_goal(
        &self,
        project_id: &str,
        _milestone_id: &str,
        _phase_id: &str,
        goal: GoalRef,
    ) -> Result<(), RoadmapError> {
        self.sql_store
            .insert_goal(
                project_id,
                &goal.goal_id,
                _phase_id,
                &goal.description,
                &goal.status.to_string(),
            )
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// 更新特定目标的状态。
    pub async fn update_goal_status(
        &self,
        project_id: &str,
        _milestone_id: &str,
        _phase_id: &str,
        goal_id: &str,
        status: MilestoneStatus,
    ) -> Result<(), RoadmapError> {
        self.sql_store
            .update_goal_status(project_id, goal_id, &status.to_string())
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    // ── 进度 ────────────────────────────────────────────────────

    /// 计算项目总体进度，返回比率 [0.0, 1.0]。
    ///
    /// 进度由所有里程碑等权计算：
    /// 每个里程碑的进度贡献是该里程碑内
    /// 已完成目标所占比例。
    pub async fn calculate_progress(&self, project_id: &str) -> Result<f64, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;
        let milestones = &project.roadmap.milestones;
        if milestones.is_empty() {
            return Ok(0.0);
        }
        let total: f64 = milestones.iter().map(|m| m.calculate_progress()).sum();
        Ok(total / milestones.len() as f64)
    }

    /// 获取项目的路线图（全部里程碑）。
    pub async fn get_roadmap(&self, project_id: &str) -> Result<Option<Roadmap>, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;
        Ok(Some(project.roadmap))
    }

    // ── 内部辅助 ────────────────────────────────────────────

    /// 获取项目，不存在时返回 `ProjectNotFound` 错误。
    pub async fn get_project_or_err(&self, id: &str) -> Result<Project, RoadmapError> {
        self.get_project(id)
            .await?
            .ok_or_else(|| RoadmapError::ProjectNotFound(id.to_string()))
    }

    /// 将结构化战略提案动作应用到路线图。
    pub async fn apply_proposal_action(
        &self,
        project_id: &str,
        action: &ProposalAction,
    ) -> Result<(), RoadmapError> {
        match action {
            ProposalAction::InsertMilestone {
                name,
                description: _description,
                after_milestone_id: _after_milestone_id,
            } => {
                let milestone_id = format!(
                    "ms-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                );

                let milestone = Milestone {
                    id: milestone_id,
                    name: name.clone(),
                    status: MilestoneStatus::Pending,
                    phases: vec![],
                };

                self.add_milestone(project_id, milestone).await?;

                // 注意：基于位置的插入（after_milestone_id）需要
                // 更新 project_milestones_key 列表顺序。
                Ok(())
            }
            ProposalAction::DeleteGoal {
                goal_id,
                rationale: _,
            } => self.delete_goal(project_id, goal_id).await,
            ProposalAction::Reprioritize {
                target_id,
                new_priority,
            } => {
                self.update_goal_priority(project_id, target_id, new_priority)
                    .await
            }
            ProposalAction::PivotTrajectory {
                new_trajectory,
                rationale: _,
            } => self.pivot_trajectory(project_id, new_trajectory).await,
        }
    }

    /// 从路线图删除目标并更新其父阶段。
    pub async fn delete_goal(&self, project_id: &str, goal_id: &str) -> Result<(), RoadmapError> {
        self.sql_store
            .delete_goal(project_id, goal_id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// Get goal description.
    pub async fn get_goal_description(
        &self,
        project_id: &str,
        goal_id: &str,
    ) -> Result<Option<String>, RoadmapError> {
        self.sql_store
            .get_goal_description(project_id, goal_id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// 更新目标优先级（简化实现）。
    pub async fn update_goal_priority(
        &self,
        project_id: &str,
        goal_id: &str,
        new_priority: &str,
    ) -> Result<(), RoadmapError> {
        // 在当前 GoalRef 中，优先级不是一等字段。
        // 我们通过更新描述或记录日志来模拟。
        tracing::info!(project_id, goal_id, new_priority, "Goal reprioritised");
        Ok(())
    }

    /// 更新项目元数据以反映战略轨迹转向。
    pub async fn pivot_trajectory(
        &self,
        project_id: &str,
        new_trajectory: &str,
    ) -> Result<(), RoadmapError> {
        self.sql_store
            .update_project_trajectory(project_id, new_trajectory)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// 从存储加载项目索引。
    /// 将数据从旧版 MfsStore（JSON 块）迁移到结构化 RoadmapSqlStore。
    pub async fn migrate_to_sql(&self, sql_store: &RoadmapSqlStore) -> Result<usize> {
        let project_ids = self
            .store
            .list_keys_with_prefix(None, "roadmap:project:")
            .await?
            .into_iter()
            .filter(|k| {
                !k.contains(":milestones")
                    && !k.contains(":milestone:")
                    && !k.contains(":phase:")
                    && !k.contains(":goals")
            })
            .collect::<Vec<_>>();

        let mut migrated_count = 0;

        for pid in project_ids {
            if let Some(project) = self.get_project(&pid).await? {
                sql_store.insert_project(&project).await?;
                for milestone in &project.roadmap.milestones {
                    sql_store
                        .insert_milestone(
                            &milestone.id,
                            &pid,
                            &milestone.name,
                            &milestone.status.to_string(),
                        )
                        .await?;
                    for phase in &milestone.phases {
                        let deps = phase.depends_on.join(",");
                        sql_store
                            .insert_phase(&pid, &phase.id, &milestone.id, &phase.name, Some(deps))
                            .await?;
                        for goal in &phase.goals {
                            sql_store
                                .insert_goal(
                                    &pid,
                                    &goal.goal_id,
                                    &phase.id,
                                    &goal.description,
                                    &goal.status.to_string(),
                                )
                                .await?;
                        }
                    }
                }
                migrated_count += 1;
            }
        }
        Ok(migrated_count)
    }
}

// ── 治理循环（自动化战略反思） ────────────────

/// 自动化治理循环的配置。
#[derive(Clone)]
pub struct GovernanceLoopConfig {
    /// 治理循环运行间隔。
    pub interval: Duration,
    /// 自动提出转向的最低置信度阈值。
    pub min_confidence: f64,
    /// 是否自动提交提案（而非仅记录日志）。
    pub auto_submit: bool,
    /// 用于战略反思的 LLM 客户端。
    pub llm: Arc<dyn LlmClient>,
}

impl std::fmt::Debug for GovernanceLoopConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GovernanceLoopConfig")
            .field("interval", &self.interval)
            .field("min_confidence", &self.min_confidence)
            .field("auto_submit", &self.auto_submit)
            .field("llm", &"<dyn LlmClient>")
            .finish()
    }
}

/// 默认配置要求提供 LLM 客户端。
impl Default for GovernanceLoopConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(3600), // 1 hour default
            min_confidence: 0.6,
            auto_submit: false, // Require human approval by default
            llm: Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
        }
    }
}

/// 单个项目治理迭代的结果。
#[derive(Debug)]
pub struct SingleProjectGovernanceResult {
    pub health: RoadmapHealth,
    pub reflection: StrategicReflection,
    pub proposal_submitted: bool,
    pub proposal_id: Option<String>,
    pub refinements: Vec<crate::milestone_generator::RefinementSuggestion>,
}

/// 单项目治理迭代的结果（含潜在错误）。
#[derive(Debug)]
pub struct GovernanceResult {
    pub project_id: String,
    pub result: Result<SingleProjectGovernanceResult, RoadmapError>,
}

/// 控制运行中治理循环的句柄。
#[derive(Debug)]
pub struct GovernanceLoopHandle {
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl GovernanceLoopHandle {
    /// 停止治理循环。
    pub async fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.task_handle.take() {
            let _ = handle.await;
        }
    }
}

impl RoadmapManager {
    /// 对所有项目运行一次治理循环迭代。
    ///
    /// 对每个项目：
    /// 1. 评估健康度
    /// 2. 运行战略反思
    /// 3. 若置信度 >= 阈值且有建议，提出转向
    /// 4. 记录结果日志
    pub async fn run_governance_iteration(
        &self,
        config: &GovernanceLoopConfig,
    ) -> Result<Vec<GovernanceResult>, RoadmapError> {
        let project_ids = self.list_project_ids().await?;
        let mut results = Vec::with_capacity(project_ids.len());

        let reflector = StrategicReflector::new(config.llm.clone());
        let generator = MilestoneGenerator::new(config.llm.clone());

        for project_id in project_ids {
            let result = self
                .run_single_project_governance(&project_id, &reflector, &generator, config)
                .await;
            results.push(GovernanceResult {
                project_id: project_id.clone(),
                result,
            });
        }

        Ok(results)
    }

    /// 为单个项目运行治理循环。
    async fn run_single_project_governance(
        &self,
        project_id: &str,
        reflector: &StrategicReflector,
        generator: &MilestoneGenerator,
        config: &GovernanceLoopConfig,
    ) -> Result<SingleProjectGovernanceResult, RoadmapError> {
        info!(project_id, "Starting governance iteration");

        // 1. 评估健康度
        let health = self.evaluate_health(project_id).await?;

        // 2. 运行战略反思
        let reflection = match reflector.reflect(self, project_id).await {
            Ok(r) => r,
            Err(e) => {
                warn!(project_id, error = %e, "Strategic reflection failed");
                return Err(e);
            }
        };

        info!(
            project_id,
            healthy = health.healthy,
            trajectory = %reflection.trajectory,
            confidence = reflection.confidence,
            "Strategic reflection completed"
        );

        // 3. 检查是否应提出转向
        let mut proposal_submitted = false;
        let mut proposal_id = None;
        let mut refinements = Vec::new();

        if reflection.confidence >= config.min_confidence && !reflection.recommendations.is_empty()
        {
            // 尝试从最高优先级建议创建提案
            if let Some(proposal) = reflector.propose_pivot(&reflection, project_id) {
                if config.auto_submit {
                    self.proposal_manager
                        .submit_proposal(project_id, proposal.clone())
                        .await?;
                    proposal_submitted = true;
                    proposal_id = Some(proposal.proposal_id.clone());
                    info!(project_id, proposal_id = %proposal.proposal_id, "Governance proposal auto-submitted");
                } else {
                    info!(project_id, proposal_id = %proposal.proposal_id, "Governance proposal generated (pending manual approval)");
                    proposal_id = Some(proposal.proposal_id);
                }
            }
        }

        // 4. 若健康度显示问题，同时尝试获取里程碑优化
        if !health.healthy {
            for ms in &health.blocked_milestones {
                if let Ok(suggestions) = generator
                    .propose_refinements(
                        self,
                        project_id,
                        ms,
                        "Milestone is blocked, needs restructuring",
                    )
                    .await
                {
                    if !suggestions.is_empty() {
                        refinements.extend(suggestions);
                    }
                }
            }
            for suggestion in &health.suggestions {
                if let Some(goal_id) = &suggestion.target_goal_id {
                    // 查找该目标所属的里程碑
                    if let Ok(Some(project)) = self.get_project(project_id).await {
                        for ms in &project.roadmap.milestones {
                            for ph in &ms.phases {
                                if ph.goals.iter().any(|g| &g.goal_id == goal_id) {
                                    if let Ok(suggestions) = generator
                                        .propose_refinements(
                                            self,
                                            project_id,
                                            &ms.id,
                                            &suggestion.problem,
                                        )
                                        .await
                                    {
                                        refinements.extend(suggestions);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 5. 若近期有失败则应用自动阻塞
        if health.recent_failures > 0 {
            let _updated_health = self.apply_blocking(project_id).await?;
        }

        Ok(SingleProjectGovernanceResult {
            health,
            reflection,
            proposal_submitted,
            proposal_id,
            refinements,
        })
    }

    /// 以后台任务启动自动化治理循环。
    ///
    /// 生成一个按配置间隔运行治理循环的 Tokio 任务。
    /// 返回可用于停止循环的句柄。
    pub async fn start_governance_loop(
        self: Arc<Self>,
        config: GovernanceLoopConfig,
    ) -> GovernanceLoopHandle {
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        let manager = self.clone();

        let handle = tokio::spawn(async move {
            let mut interval = interval(config.interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        info!("Running scheduled governance loop");
                        match manager.run_governance_iteration(&config).await {
                            Ok(results) => {
                                for result in results {
                                    match &result.result {
                                        Ok(gov_result) => {
                                            info!(
                                                project_id = %result.project_id,
                                                healthy = gov_result.health.healthy,
                                                proposal_submitted = gov_result.proposal_submitted,
                                                refinements = gov_result.refinements.len(),
                                                "Governance iteration completed"
                                            );
                                        }
                                        Err(e) => {
                                            error!(
                                                project_id = %result.project_id,
                                                error = %e,
                                                "Governance iteration failed"
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "Governance loop iteration failed globally");
                            }
                        }
                    }
                    _ = &mut rx => {
                        info!("Governance loop stopped by signal");
                        break;
                    }
                }
            }
        });

        GovernanceLoopHandle {
            stop_tx: Some(tx),
            task_handle: Some(handle),
        }
    }

    // ── 资源优化 ────────────────────────────────────────

    /// 基于历史执行结果预测剩余成本与耗时。
    pub async fn project_costs(
        &self,
        project_id: &str,
        total_goals: usize,
        completed_goals: usize,
    ) -> Result<CostProjection, RoadmapError> {
        let outcomes = self.get_outcomes(project_id).await?;
        let optimizer = ResourceOptimizer::new();
        Ok(optimizer.project_costs(&outcomes, total_goals, completed_goals))
    }

    /// 识别项目执行流水线中的瓶颈。
    pub async fn find_bottlenecks(
        &self,
        project_id: &str,
    ) -> Result<Vec<Bottleneck>, RoadmapError> {
        let outcomes = self.get_outcomes(project_id).await?;
        let optimizer = ResourceOptimizer::new();
        Ok(optimizer.find_bottlenecks(&outcomes))
    }

    /// 建议可并行执行的目标。
    pub async fn suggest_parallel_execution(
        &self,
        project_id: &str,
    ) -> Result<Vec<ParallelizationOpportunity>, RoadmapError> {
        let outcomes = self.get_outcomes(project_id).await?;
        let optimizer = ResourceOptimizer::new();
        Ok(optimizer.suggest_parallel_execution(&outcomes))
    }

    /// 为一组目标 ID 生成逐目标资源估算。
    pub async fn estimate_goal_resources(
        &self,
        project_id: &str,
        goal_ids: &[String],
    ) -> Result<Vec<GoalResourceEstimate>, RoadmapError> {
        let outcomes = self.get_outcomes(project_id).await?;
        let optimizer = ResourceOptimizer::new();
        Ok(optimizer.estimate_goal_resources(&outcomes, goal_ids))
    }
}
