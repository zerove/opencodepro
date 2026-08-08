// ---------------------------------------------------------------------------
// 阶段 3 — 战略反思循环
//
// 运行在战术认知循环之上的高阶反思层。
// 它不分析单次 SOP 执行轨迹，而是审视
// **完整路线图轨迹** — 里程碑进展、执行结果
// 模式、健康趋势 — 并产出 LLM 增强的战略洞察
// 与可执行建议。
// ---------------------------------------------------------------------------

use crate::{ExecutionOutcome, RoadmapError, RoadmapManager};
use serde::{Deserialize, Serialize};
use sisyphus_tools_llm::metis::LlmClient;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 反思引擎产出的单条战略建议。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicRecommendation {
    /// 简短行动说明（例如 "Reprioritise milestone 'Data Ingestion'"）。
    pub action: String,
    /// 需要此行动的原因，附证据支撑。
    pub rationale: String,
    /// 优先级："high" / "medium" / "low"。
    pub priority: String,
    /// 此建议适用的可选目标里程碑 ID。
    pub target_milestone: Option<String>,
}

/// 项目的完整战略反思报告。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicReflection {
    pub project_id: String,
    pub project_name: String,
    /// LLM 撰写的总体健康评估（自由文本）。
    pub health_assessment: String,
    /// 轨迹分析 — 项目处于加速、停滞等状态。
    pub trajectory: String,
    /// 已识别的风险因素。
    pub risk_factors: Vec<String>,
    /// 具体建议，按优先级排序。
    pub recommendations: Vec<StrategicRecommendation>,
    /// 分析的置信度 [0.0, 1.0]。
    pub confidence: f64,
}

/// 修改项目路线图的结构化动作。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProposalAction {
    /// 在特定位置插入新里程碑。
    InsertMilestone {
        name: String,
        description: String,
        after_milestone_id: Option<String>,
    },
    /// 删除不再相关的目标。
    DeleteGoal { goal_id: String, rationale: String },
    /// 更改目标或里程碑的优先级。
    Reprioritize {
        target_id: String,
        new_priority: String,
    },
    /// 一般性战略转向（例如改变项目总体方向）。
    PivotTrajectory {
        new_trajectory: String,
        rationale: String,
    },
}

/// 项目路线图战略转向的正式提案。
/// 这是人在回路审批的主要载体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicProposal {
    pub proposal_id: String,
    pub project_id: String,
    pub timestamp: u64,
    /// 触发此提案时的路线图当前状态。
    pub trigger_reflection: StrategicReflection,
    /// 批准后要执行的结构化动作。
    pub action: ProposalAction,
    /// 对项目健康度/轨迹的预期影响。
    pub expected_impact: String,
    /// 当前状态："Pending" / "Approved" / "Rejected"。
    pub status: ProposalStatus,
    /// 批准/拒绝此提案的架构师引用。
    pub reviewer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalStatus {
    Pending,
    Approved,
    Rejected,
}

// ---------------------------------------------------------------------------
// StrategicReflector（战略反思器）
// ---------------------------------------------------------------------------

/// 消费路线图数据 + 执行历史并产出
/// LLM 增强战略反思的引擎。
pub struct StrategicReflector {
    llm: Arc<dyn LlmClient>,
}

impl StrategicReflector {
    /// 创建由给定 LLM 客户端支撑的反思器。
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    /// 为指定项目运行战略反思。
    ///
    /// 收集进度报告、健康评估与近期执行结果，
    /// 然后提供给 LLM 进行深度分析。
    pub async fn reflect(
        &self,
        manager: &RoadmapManager,
        project_id: &str,
    ) -> Result<StrategicReflection, RoadmapError> {
        // 1. 从路线图管理器收集结构化数据
        let report = manager.generate_progress_report(project_id).await?;
        let health = manager.evaluate_health(project_id).await?;
        let outcomes = manager.get_outcomes(project_id).await?;

        // 2. 为 LLM 构建紧凑的上下文字符串
        let context = self.build_context(&report, &health, &outcomes);

        // 3. 调用 LLM
        let system = "\
You are a senior engineering director analysing a project roadmap. \
Given the project's progress breakdown, health evaluation, and recent execution outcomes, \
produce a strategic reflection with:

1. **health_assessment**: A concise (2-3 sentence) overall health assessment.
2. **trajectory**: Whether the project is on track, accelerating, stagnating, or at risk.
3. **risk_factors**: An array of specific risk factors (strings).
4. **recommendations**: An array of { action, rationale, priority, target_milestone } objects.
   Priorities are \"high\" / \"medium\" / \"low\". target_milestone is optional.
5. **confidence**: A number between 0 and 1 indicating your confidence in this analysis.

Respond ONLY with valid JSON matching this structure:
{
  \"health_assessment\": \"...\",
  \"trajectory\": \"...\",
  \"risk_factors\": [\"...\"],
  \"recommendations\": [
    { \"action\": \"...\", \"rationale\": \"...\", \"priority\": \"high\", \"target_milestone\": null }
  ],
  \"confidence\": 0.85
}";

        let response = self
            .llm
            .chat(system, &context, true)
            .await
            .map_err(|e| RoadmapError::Internal(format!("LLM reflection failed: {e}")))?;

        // 4. 解析 LLM 响应
        let parsed: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| RoadmapError::Internal(format!("Failed to parse reflection JSON: {e}")))?;

        let health_assessment = parsed["health_assessment"]
            .as_str()
            .unwrap_or("Unable to assess health")
            .to_string();
        let trajectory = parsed["trajectory"]
            .as_str()
            .unwrap_or("Unable to determine trajectory")
            .to_string();
        let risk_factors: Vec<String> = parsed["risk_factors"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let recommendations: Vec<StrategicRecommendation> = parsed["recommendations"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(StrategicRecommendation {
                            action: v["action"].as_str()?.to_string(),
                            rationale: v["rationale"].as_str()?.to_string(),
                            priority: v["priority"].as_str().unwrap_or("medium").to_string(),
                            target_milestone: v["target_milestone"].as_str().map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let confidence = parsed["confidence"].as_f64().unwrap_or(0.5);

        Ok(StrategicReflection {
            project_id: project_id.to_string(),
            project_name: report.project_name,
            health_assessment,
            trajectory,
            risk_factors,
            recommendations,
            confidence,
        })
    }

    /// 将战略反思转化为路线图转向的正式提案。
    pub fn propose_pivot(
        &self,
        reflection: &StrategicReflection,
        project_id: &str,
    ) -> Option<StrategicProposal> {
        // 仅在存在具体建议且置信度可接受时提出转向。
        let best_rec = reflection.recommendations.first()?;
        if reflection.confidence < 0.6 {
            return None;
        }

        Some(StrategicProposal {
            proposal_id: format!(
                "prop-{}-{}",
                project_id,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ),
            project_id: project_id.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            trigger_reflection: reflection.clone(),
            action: ProposalAction::PivotTrajectory {
                new_trajectory: best_rec.action.clone(),
                rationale: best_rec.rationale.clone(),
            },
            expected_impact: best_rec.rationale.clone(),
            status: ProposalStatus::Pending,
            reviewer: None,
        })
    }

    // ── 内部辅助 ────────────────────────────────────────────

    /// 从路线图数据构建紧凑的上下文字符串。
    fn build_context(
        &self,
        report: &crate::ProgressReport,
        health: &crate::RoadmapHealth,
        outcomes: &[ExecutionOutcome],
    ) -> String {
        let mut ctx = String::with_capacity(2048);

        ctx.push_str(&format!(
            "Project: {} ({})\n",
            report.project_name, report.project_id
        ));
        ctx.push_str(&format!(
            "Progress: {:.1}% ({}/{})\n",
            report.overall_progress * 100.0,
            report.completed_goals,
            report.total_goals
        ));
        ctx.push_str(&format!("Milestones: {} total\n", report.milestone_count));

        // 逐里程碑明细
        ctx.push_str("\n--- Milestones ---\n");
        for ms in &report.milestones {
            ctx.push_str(&format!(
                "  {}: {:.1}% ({}/{}), status={:?}\n",
                ms.name,
                ms.progress * 100.0,
                ms.completed_count,
                ms.goal_count,
                ms.status
            ));
            for ph in &ms.phases {
                ctx.push_str(&format!(
                    "    Phase {}: {:.1}% ({}/{})\n",
                    ph.name,
                    ph.progress * 100.0,
                    ph.completed_count,
                    ph.goal_count
                ));
            }
        }

        // 健康度摘要
        ctx.push_str("\n--- Health ---\n");
        ctx.push_str(&format!("Healthy: {}\n", health.healthy));
        ctx.push_str(&format!(
            "Blocked milestones: {}\n",
            health.blocked_milestones.join(", ")
        ));
        ctx.push_str(&format!("Recent failures: {}\n", health.recent_failures));
        if !health.problems.is_empty() {
            ctx.push_str("Problems:\n");
            for p in &health.problems {
                ctx.push_str(&format!("  [{:?}] {}\n", p.severity, p.message));
            }
        }

        // 执行结果摘要
        ctx.push_str("\n--- Recent Outcomes ---\n");
        let recent: Vec<&ExecutionOutcome> = outcomes.iter().rev().take(20).collect();
        for o in &recent {
            ctx.push_str(&format!(
                "  goal={} success={} dev={:.2} err={:?}\n",
                o.goal_id,
                o.success,
                o.deviation_score.unwrap_or(0.0),
                o.error.as_ref().map(|e| &e[..e.len().min(60)])
            ));
        }

        ctx
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RoadmapManager;
    use sisyphus_mem::MfsStore;
    use sisyphus_tools_llm::metis::FixedResponseClient;
    use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase};
    use std::sync::Arc;

    fn make_goal(id: &str, desc: &str, status: MilestoneStatus) -> GoalRef {
        GoalRef {
            goal_id: id.to_string(),
            description: desc.to_string(),
            status,
        }
    }

    fn make_phase(id: &str, name: &str, goals: Vec<GoalRef>) -> Phase {
        Phase {
            id: id.to_string(),
            name: name.to_string(),
            goals,
            depends_on: vec![],
        }
    }

    fn make_milestone(
        id: &str,
        name: &str,
        phases: Vec<Phase>,
        status: MilestoneStatus,
    ) -> Milestone {
        Milestone {
            id: id.to_string(),
            name: name.to_string(),
            phases,
            status,
        }
    }

    async fn setup_manager() -> (RoadmapManager, Arc<MfsStore>) {
        let store = Arc::new(
            MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
                .await
                .expect("Failed to create MfsStore"),
        );
        let sql_store = Arc::new(
            sisyphus_mem::RoadmapSqlStore::new(store.pool().clone())
                .await
                .expect("Failed to create sql store"),
        );
        sql_store
            .init_schema()
            .await
            .expect("Failed to init schema");
        let proposal_manager =
            Arc::new(crate::proposal_manager::ProposalManager::new(store.clone()));
        (
            RoadmapManager::new(store.clone(), sql_store, proposal_manager),
            store,
        )
    }

    #[tokio::test]
    async fn test_reflect_empty_project() {
        let (mgr, _store) = setup_manager().await;
        mgr.create_project("empty", "Empty Project", "desc")
            .await
            .unwrap();

        let llm = Arc::new(FixedResponseClient(
            r#"{
                "health_assessment": "Project is empty, no work tracked.",
                "trajectory": "Not started",
                "risk_factors": ["No milestones defined"],
                "recommendations": [
                    {
                        "action": "Define initial milestones",
                        "rationale": "No work breakdown exists",
                        "priority": "high",
                        "target_milestone": null
                    }
                ],
                "confidence": 0.9
            }"#
            .to_string(),
        ));

        let reflector = StrategicReflector::new(llm);
        let reflection = reflector.reflect(&mgr, "empty").await.unwrap();

        assert_eq!(reflection.project_id, "empty");
        assert!(reflection.health_assessment.contains("empty"));
        assert_eq!(reflection.recommendations.len(), 1);
        assert_eq!(reflection.recommendations[0].priority, "high");
    }

    #[tokio::test]
    async fn test_reflect_with_milestones() {
        let (mgr, _store) = setup_manager().await;
        mgr.create_project("p1", "Active Project", "desc")
            .await
            .unwrap();

        let g1 = make_goal("g1", "Setup infra", MilestoneStatus::Completed);
        let g2 = make_goal("g2", "Build API", MilestoneStatus::InProgress);
        let g3 = make_goal("g3", "Write docs", MilestoneStatus::Pending);
        let ph = make_phase("ph1", "Dev", vec![g1, g2, g3]);
        let ms = make_milestone("ms1", "Release v1", vec![ph], MilestoneStatus::InProgress);
        mgr.add_milestone("p1", ms).await.unwrap();

        let llm = Arc::new(FixedResponseClient(
            r#"{
                "health_assessment": "Project is progressing steadily.",
                "trajectory": "On track",
                "risk_factors": ["Documentation not started"],
                "recommendations": [
                    {
                        "action": "Prioritize documentation",
                        "rationale": "Docs are the only pending item",
                        "priority": "medium",
                        "target_milestone": "ms1"
                    }
                ],
                "confidence": 0.8
            }"#
            .to_string(),
        ));

        let reflector = StrategicReflector::new(llm);
        let reflection = reflector.reflect(&mgr, "p1").await.unwrap();

        assert_eq!(reflection.project_name, "Active Project");
        assert_eq!(reflection.risk_factors.len(), 1);
        assert_eq!(
            reflection.recommendations[0].target_milestone.as_deref(),
            Some("ms1")
        );
    }

    #[tokio::test]
    async fn test_reflect_llm_failure_returns_error() {
        let (mgr, _store) = setup_manager().await;
        mgr.create_project("fail", "Fail", "desc").await.unwrap();

        // 始终失败的 LLM
        struct FailLlm;
        #[async_trait::async_trait]
        impl LlmClient for FailLlm {
            async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError> {
                self.chat("", prompt, false).await
            }

            async fn chat(
                &self,
                _: &str,
                _: &str,
                _: bool,
            ) -> Result<String, sisyphus_types::TypesError> {
                Err(sisyphus_types::TypesError::LlmError("LLM down".to_string()))
            }

            fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
                (
                    "fail_key".into(),
                    "http://fail:8080".into(),
                    "fail-model".into(),
                )
            }

            fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
                sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
            }
        }

        let reflector = StrategicReflector::new(Arc::new(FailLlm));
        let err = reflector.reflect(&mgr, "fail").await.unwrap_err();
        assert!(err.to_string().contains("LLM reflection failed"));
    }

    #[tokio::test]
    async fn test_build_context_includes_all_sections() {
        let (mgr, _store) = setup_manager().await;
        mgr.create_project("ctx", "Ctx", "desc").await.unwrap();
        let report = mgr.generate_progress_report("ctx").await.unwrap();
        let health = mgr.evaluate_health("ctx").await.unwrap();
        let outcomes = mgr.get_outcomes("ctx").await.unwrap();

        let llm = Arc::new(FixedResponseClient("{}".to_string()));
        let reflector = StrategicReflector::new(llm);
        let context = reflector.build_context(&report, &health, &outcomes);

        assert!(context.contains("Ctx"));
        assert!(context.contains("Progress:"));
        assert!(context.contains("Milestones:"));
        assert!(context.contains("Healthy:"));
    }
}
