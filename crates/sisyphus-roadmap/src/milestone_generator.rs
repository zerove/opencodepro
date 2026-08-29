// ---------------------------------------------------------------------------
// 阶段 3 — 自主里程碑生成
//
// 使用 LLM 根据项目描述提出最优的里程碑 / 阶段 / 目标
// 拆解方案，并能基于执行结果与战略反思建议
// 优化现有路线图。
// ---------------------------------------------------------------------------

use crate::{RoadmapError, RoadmapManager};
use serde::{Deserialize, Serialize};
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// LLM 生成器提出的里程碑。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MilestoneProposal {
    pub id: String,
    pub name: String,
    pub phases: Vec<PhaseProposal>,
}

/// 里程碑提案中的阶段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseProposal {
    pub id: String,
    pub name: String,
    pub goals: Vec<GoalProposal>,
}

/// 阶段提案中的目标。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalProposal {
    pub id: String,
    pub description: String,
}

/// 项目的完整路线图提案。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoadmapProposal {
    pub project_name: String,
    pub milestones: Vec<MilestoneProposal>,
}

/// 对路线图中单个元素的优化建议。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefinementSuggestion {
    pub milestone_id: Option<String>,
    pub phase_id: Option<String>,
    pub action: String,
    pub rationale: String,
}

// ---------------------------------------------------------------------------
// MilestoneGenerator（里程碑生成器）
// ---------------------------------------------------------------------------

/// 使用 LLM 生成与优化项目路线图的引擎。
pub struct MilestoneGenerator {
    llm: Arc<dyn LlmClient>,
}

impl MilestoneGenerator {
    /// New.
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    /// 从自然语言项目描述生成整套里程碑，
    /// 并持久化到指定项目。
    ///
    /// 返回创建的里程碑列表。
    pub async fn generate_milestones(
        &self,
        manager: &RoadmapManager,
        project_id: &str,
        description: &str,
    ) -> Result<Vec<Milestone>, RoadmapError> {
        let system = "\
You are a project planning expert. Given a project description, generate a structured \
roadmap with milestones, phases, and goals. Follow these rules:

- Produce 3-5 milestones with 1-3 phases each and 2-5 goals per phase.
- Make goals concrete, actionable, and atomic (single unit of work).
- Each goal should be completable in a single session.
- Use meaningful IDs (e.g. \"m1\", \"m1_p1\", \"m1_p1_g1\").
- Phases within a milestone should be ordered by logical dependency.

Respond ONLY with valid JSON matching this structure:
{
  \"project_name\": \"...\",
  \"milestones\": [
    {
      \"id\": \"m1\",
      \"name\": \"Milestone Name\",
      \"phases\": [
        {
          \"id\": \"m1_p1\",
          \"name\": \"Phase Name\",
          \"goals\": [
            { \"id\": \"m1_p1_g1\", \"description\": \"Goal description\" }
          ]
        }
      ]
    }
  ]
}";

        let response = self
            .llm
            .chat(system, description, true)
            .await
            .map_err(|e| RoadmapError::Internal(format!("LLM milestone generation failed: {e}")))?;

        let proposal: RoadmapProposal = serde_json::from_str(&response).map_err(|e| {
            RoadmapError::Internal(format!(
                "Failed to parse milestone proposal JSON: {e}. Raw: {response}"
            ))
        })?;

        // 将提案转换为 Milestone 对象并持久化
        let mut milestones = Vec::with_capacity(proposal.milestones.len());
        for mp in proposal.milestones {
            let mut phases = Vec::with_capacity(mp.phases.len());
            for pp in mp.phases {
                let goals: Vec<GoalRef> = pp
                    .goals
                    .into_iter()
                    .map(|gp| GoalRef {
                        goal_id: gp.id,
                        description: gp.description,
                        status: MilestoneStatus::Pending,
                    })
                    .collect();
                phases.push(Phase {
                    id: pp.id,
                    name: pp.name,
                    depends_on: vec![],
                    goals,
                });
            }

            let milestone = Milestone {
                id: mp.id,
                name: mp.name,
                status: MilestoneStatus::Pending,
                phases,
            };

            manager.add_milestone(project_id, milestone.clone()).await?;
            milestones.push(milestone);
        }

        Ok(milestones)
    }

    /// 基于执行结果与变更请求，优化现有里程碑的
    /// 阶段/目标拆解。
    ///
    /// 返回优化建议列表而非直接修改，
    /// 供调用方审查后应用。
    pub async fn propose_refinements(
        &self,
        manager: &RoadmapManager,
        project_id: &str,
        milestone_id: &str,
        change_request: &str,
    ) -> Result<Vec<RefinementSuggestion>, RoadmapError> {
        // 加载当前项目状态作为上下文
        let project = manager
            .get_project(project_id)
            .await?
            .ok_or_else(|| RoadmapError::ProjectNotFound(project_id.to_string()))?;

        let milestone = project
            .roadmap
            .milestones
            .iter()
            .find(|m| m.id == milestone_id)
            .ok_or_else(|| {
                RoadmapError::MilestoneNotFound(milestone_id.to_string(), project_id.to_string())
            })?;

        // 获取该里程碑的近期执行结果
        let outcomes = manager.get_outcomes(project_id).await?;
        let relevant_outcomes: Vec<_> = outcomes
            .iter()
            .filter(|o| o.milestone_id == milestone_id)
            .collect();

        // 构建上下文
        let mut context = format!(
            "## Current Milestone: {} ({})\n\nPhases and Goals:\n",
            milestone.name, milestone.id
        );
        for ph in &milestone.phases {
            context.push_str(&format!("  Phase: {} ({})\n", ph.name, ph.id));
            for g in &ph.goals {
                context.push_str(&format!(
                    "    Goal: {} [{}] {:?}\n",
                    g.description, g.goal_id, g.status
                ));
            }
        }

        if !relevant_outcomes.is_empty() {
            context.push_str("\nRecent execution outcomes:\n");
            for o in &relevant_outcomes {
                context.push_str(&format!(
                    "  goal={} success={} dev={:.2}\n",
                    o.goal_id,
                    o.success,
                    o.deviation_score.unwrap_or(0.0),
                ));
            }
        }

        context.push_str(&format!("\nChange request: {change_request}\n"));

        let system = "\
You are a project planning expert. Given the current state of a milestone and its recent \
execution outcomes, propose refinements to improve the milestone's structure. \
Focus on:

- Re-ordering goals to prioritise blocked or high-risk items.
- Splitting overly large goals.
- Merging duplicate or overlapping goals.
- Adding missing goals revealed by execution patterns.

Respond ONLY with valid JSON as an array of refinement suggestions:
[
  {
    \"milestone_id\": \"...\",
    \"phase_id\": \"...\" | null,
    \"action\": \"Description of what to change\",
    \"rationale\": \"Why this change is needed\"
  }
]";

        let response = self
            .llm
            .chat(system, &context, true)
            .await
            .map_err(|e| RoadmapError::Internal(format!("LLM refinement failed: {e}")))?;

        let suggestions: Vec<RefinementSuggestion> =
            serde_json::from_str(&response).map_err(|e| {
                RoadmapError::Internal(format!(
                    "Failed to parse refinement JSON: {e}. Raw: {response}"
                ))
            })?;

        Ok(suggestions)
    }

    /// 通过分析项目描述中缺失的里程碑，自动填补
    /// 项目路线图中的空白。
    pub async fn fill_roadmap_gaps(
        &self,
        manager: &RoadmapManager,
        project_id: &str,
        project_description: &str,
    ) -> Result<Vec<Milestone>, RoadmapError> {
        let project = manager
            .get_project(project_id)
            .await?
            .ok_or_else(|| RoadmapError::ProjectNotFound(project_id.to_string()))?;

        let existing_milestones: Vec<String> = project
            .roadmap
            .milestones
            .iter()
            .map(|m| format!("- {} ({}): {} phases", m.name, m.id, m.phases.len()))
            .collect();

        let mut context =
            format!("Project description: {project_description}\n\nAlready defined milestones:\n");
        if existing_milestones.is_empty() {
            context.push_str("  (none)\n");
        } else {
            for m in &existing_milestones {
                context.push_str(&format!("  {m}\n"));
            }
        }
        context.push_str(
            "\nIdentify gaps in this roadmap and propose additional milestones \
             that would meaningfully complete the project scope. Avoid duplicating \
             existing milestones.",
        );

        let system = "\
You are a project planning expert. Given a project description and its existing milestones, \
identify missing milestones that are necessary to complete the project scope. \
Propose 1-3 additional milestones.

Respond ONLY with valid JSON matching the standard roadmap proposal format:
{
  \"project_name\": \"...\",
  \"milestones\": [
    {
      \"id\": \"m_new_1\",
      \"name\": \"Missing Milestone\",
      \"phases\": [...]
    }
  ]
}";

        let response = self
            .llm
            .chat(system, &context, true)
            .await
            .map_err(|e| RoadmapError::Internal(format!("LLM gap fill failed: {e}")))?;

        let proposal: RoadmapProposal = serde_json::from_str(&response).map_err(|e| {
            RoadmapError::Internal(format!(
                "Failed to parse gap-fill JSON: {e}. Raw: {response}"
            ))
        })?;

        let mut milestones = Vec::with_capacity(proposal.milestones.len());
        for mp in proposal.milestones {
            let mut phases = Vec::with_capacity(mp.phases.len());
            for pp in mp.phases {
                let goals: Vec<GoalRef> = pp
                    .goals
                    .into_iter()
                    .map(|gp| GoalRef {
                        goal_id: gp.id,
                        description: gp.description,
                        status: MilestoneStatus::Pending,
                    })
                    .collect();
                phases.push(Phase {
                    id: pp.id,
                    name: pp.name,
                    depends_on: vec![],
                    goals,
                });
            }

            let milestone = Milestone {
                id: mp.id,
                name: mp.name,
                status: MilestoneStatus::Pending,
                phases,
            };

            manager.add_milestone(project_id, milestone.clone()).await?;
            milestones.push(milestone);
        }

        Ok(milestones)
    }
}

// ---------------------------------------------------------------------------
#[cfg(test)]
#[path = "milestone_generator_tests.rs"]
mod tests;
