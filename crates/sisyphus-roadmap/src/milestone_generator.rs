// ---------------------------------------------------------------------------
// Phase 3 — Autonomous Milestone Generation
//
// Uses LLM to propose optimal milestone / phase / goal breakdowns from a
// project description, and can refine existing roadmaps based on execution
// outcomes and strategic reflection recommendations.
// ---------------------------------------------------------------------------

use crate::{RoadmapError, RoadmapManager};
use serde::{Deserialize, Serialize};
use sisyphus_tools::metis::LlmClient;
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A proposed milestone from the LLM generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MilestoneProposal {
    pub id: String,
    pub name: String,
    pub phases: Vec<PhaseProposal>,
}

/// A proposed phase within a milestone proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseProposal {
    pub id: String,
    pub name: String,
    pub goals: Vec<GoalProposal>,
}

/// A proposed goal within a phase proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalProposal {
    pub id: String,
    pub description: String,
}

/// Full roadmap proposal for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoadmapProposal {
    pub project_name: String,
    pub milestones: Vec<MilestoneProposal>,
}

/// A refinement suggestion for a single element in the roadmap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefinementSuggestion {
    pub milestone_id: Option<String>,
    pub phase_id: Option<String>,
    pub action: String,
    pub rationale: String,
}

// ---------------------------------------------------------------------------
// MilestoneGenerator
// ---------------------------------------------------------------------------

/// Engine that uses LLM to generate and refine project roadmaps.
pub struct MilestoneGenerator {
    llm: Arc<dyn LlmClient>,
}

impl MilestoneGenerator {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    /// Generate a full set of milestones from a natural-language project
    /// description, and persist them into the given project.
    ///
    /// Returns the list of created milestones.
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

        // Convert proposal to Milestone objects and persist
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

    /// Refine an existing milestone's phase/goal breakdown based on
    /// execution outcomes and a change request.
    ///
    /// Returns a list of refinement suggestions rather than mutating directly,
    /// so the caller can review and apply them.
    pub async fn propose_refinements(
        &self,
        manager: &RoadmapManager,
        project_id: &str,
        milestone_id: &str,
        change_request: &str,
    ) -> Result<Vec<RefinementSuggestion>, RoadmapError> {
        // Load current project state for context
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

        // Get recent outcomes for this milestone
        let outcomes = manager.get_outcomes(project_id).await?;
        let relevant_outcomes: Vec<_> = outcomes
            .iter()
            .filter(|o| o.milestone_id == milestone_id)
            .collect();

        // Build context
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

    /// Automatically fill gaps in a project's roadmap by analysing what
    /// milestones are missing based on the project description.
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RoadmapManager;
    use sisyphus_mem::MfsStore;
    use sisyphus_tools::metis::FixedResponseClient;
    use std::sync::Arc;

    async fn setup_manager() -> (RoadmapManager, Arc<MfsStore>) {
        let store = Arc::new(
            MfsStore::new("sqlite::memory:")
                .await
                .expect("Failed to create MfsStore"),
        );
        (RoadmapManager::new(store.clone()), store)
    }

    #[tokio::test]
    async fn test_generate_milestones_parses_llm_response() {
        let (mgr, _store) = setup_manager().await;
        mgr.create_project("p1", "Test Project", "A test")
            .await
            .unwrap();

        let llm = Arc::new(FixedResponseClient(
            r#"{
                "project_name": "Test Project",
                "milestones": [
                    {
                        "id": "m1",
                        "name": "Foundation",
                        "phases": [
                            {
                                "id": "m1_p1",
                                "name": "Setup",
                                "goals": [
                                    {"id": "m1_p1_g1", "description": "Install dependencies"},
                                    {"id": "m1_p1_g2", "description": "Configure CI"}
                                ]
                            }
                        ]
                    },
                    {
                        "id": "m2",
                        "name": "Core Features",
                        "phases": [
                            {
                                "id": "m2_p1",
                                "name": "Backend",
                                "goals": [
                                    {"id": "m2_p1_g1", "description": "Build API"}
                                ]
                            }
                        ]
                    }
                ]
            }"#
            .to_string(),
        ));

        let gen = MilestoneGenerator::new(llm);
        let milestones = gen
            .generate_milestones(&mgr, "p1", "Build a web application")
            .await
            .unwrap();

        assert_eq!(milestones.len(), 2);
        assert_eq!(milestones[0].name, "Foundation");
        assert_eq!(milestones[0].phases[0].goals.len(), 2);
        assert_eq!(milestones[1].phases[0].goals[0].goal_id, "m2_p1_g1");

        // Verify persisted
        let project = mgr.get_project("p1").await.unwrap().unwrap();
        assert_eq!(project.roadmap.milestones.len(), 2);
    }

    #[tokio::test]
    async fn test_generate_milestones_invalid_json() {
        let (mgr, _store) = setup_manager().await;
        mgr.create_project("p2", "Bad", "desc").await.unwrap();

        let llm = Arc::new(FixedResponseClient("not valid json".to_string()));
        let gen = MilestoneGenerator::new(llm);
        let err = gen
            .generate_milestones(&mgr, "p2", "test")
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("Failed to parse milestone proposal"));
    }

    #[tokio::test]
    async fn test_propose_refinements_returns_suggestions() {
        let (mgr, _store) = setup_manager().await;
        mgr.create_project("p3", "Refine", "desc").await.unwrap();

        // Add a milestone with goals
        use sisyphus_types::{GoalRef, MilestoneStatus};
        let goal = GoalRef {
            goal_id: "g1".into(),
            description: "Implement feature X".into(),
            status: MilestoneStatus::Blocked,
        };
        let phase = Phase {
            id: "ph1".into(),
            name: "Development".into(),
            depends_on: vec![],
            goals: vec![goal],
        };
        let ms = Milestone {
            id: "ms1".into(),
            name: "Release".into(),
            status: MilestoneStatus::InProgress,
            phases: vec![phase],
        };
        mgr.add_milestone("p3", ms).await.unwrap();

        let llm = Arc::new(FixedResponseClient(
            r#"[
                {
                    "milestone_id": "ms1",
                    "phase_id": "ph1",
                    "action": "Split blocked goal into smaller tasks",
                    "rationale": "Goal is blocked and too large"
                }
            ]"#
            .to_string(),
        ));

        let gen = MilestoneGenerator::new(llm);
        let suggestions = gen
            .propose_refinements(&mgr, "p3", "ms1", "Goal is blocked")
            .await
            .unwrap();

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].milestone_id.as_deref(), Some("ms1"));
        assert!(suggestions[0].action.contains("Split"));
    }

    #[tokio::test]
    async fn test_fill_roadmap_gaps() {
        let (mgr, _store) = setup_manager().await;
        mgr.create_project("p4", "Gap Fill", "desc").await.unwrap();

        // Add one existing milestone
        let ms = Milestone {
            id: "m1".into(),
            name: "Foundation".into(),
            status: MilestoneStatus::InProgress,
            phases: vec![],
        };
        mgr.add_milestone("p4", ms).await.unwrap();

        let llm = Arc::new(FixedResponseClient(
            r#"{
                "project_name": "Gap Fill",
                "milestones": [
                    {
                        "id": "m2",
                        "name": "Testing & QA",
                        "phases": [
                            {
                                "id": "m2_p1",
                                "name": "Test Suite",
                                "goals": [
                                    {"id": "m2_p1_g1", "description": "Write unit tests"}
                                ]
                            }
                        ]
                    }
                ]
            }"#
            .to_string(),
        ));

        let gen = MilestoneGenerator::new(llm);
        let new_ms = gen
            .fill_roadmap_gaps(&mgr, "p4", "Build a web app")
            .await
            .unwrap();

        assert_eq!(new_ms.len(), 1);
        assert_eq!(new_ms[0].name, "Testing & QA");

        // Verify both milestones exist
        let project = mgr.get_project("p4").await.unwrap().unwrap();
        assert_eq!(project.roadmap.milestones.len(), 2);
    }

    #[tokio::test]
    async fn test_generate_empty_response() {
        let (mgr, _store) = setup_manager().await;
        mgr.create_project("p5", "Empty", "desc").await.unwrap();

        let llm = Arc::new(FixedResponseClient(
            r#"{"project_name": "Empty", "milestones": []}"#.to_string(),
        ));

        let gen = MilestoneGenerator::new(llm);
        let milestones = gen.generate_milestones(&mgr, "p5", "test").await.unwrap();
        assert!(milestones.is_empty());
    }
}
