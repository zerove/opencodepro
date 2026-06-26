use sisyphus_types::{Project, Milestone, GoalRef, MilestoneStatus, StateStore};
use std::sync::Arc;

/// 项目路线图管理器，负责管理项目的宏观目标、里程碑及进度。
pub struct RoadmapManager {
    state_store: Arc<dyn StateStore>,
    project_id: String,
}

impl RoadmapManager {
    pub fn new(state_store: Arc<dyn StateStore>, project_id: String) -> Self {
        Self {
            state_store,
            project_id,
        }
    }

    /// 加载当前项目的完整状态
    pub async fn load_project(&self) -> Result<Project, String> {
        let key = format!("project:{}", self.project_id);
        let val = self.state_store.get_state_json(&key).await?
            .ok_or_else(|| format!("Project {} not found", self.project_id))?;
        
        serde_json::from_value(val).map_err(|e| format!("Failed to deserialize project: {e}"))
    }

    /// 保存项目状态
    pub async fn save_project(&self, project: &Project) -> Result<(), String> {
        let key = format!("project:{}", self.project_id);
        self.state_store.set_state_json(&key, serde_json::to_value(project).map_err(|e| e.to_string())?).await
    }

    /// 创建一个新的项目路线图
    pub async fn create_project(&self, name: String, description: String) -> Result<Project, String> {
        let project = Project::new(self.project_id.clone(), name, description);
        self.save_project(&project).await?;
        Ok(project)
    }

    /// 在路线图中添加里程碑
    pub async fn add_milestone(&self, milestone: Milestone) -> Result<(), String> {
        let mut project = self.load_project().await?;
        project.roadmap.milestones.push(milestone);
        self.save_project(&project).await
    }

    /// 在路线图中添加具体目标到特定阶段
    pub async fn add_goal_to_phase(&self, milestone_id: &str, phase_id: &str, goal: GoalRef) -> Result<(), String> {
        let mut project = self.load_project().await?;
        for milestone in &mut project.roadmap.milestones {
            if milestone.id == milestone_id {
                for phase in &mut milestone.phases {
                    if phase.id == phase_id {
                        phase.goals.push(goal);
                        return self.save_project(&project).await;
                    }
                }
            }
        }
        Err(format!("Milestone {milestone_id} or Phase {phase_id} not found"))
    }

    pub async fn remove_goal(&self, goal_id: &str) -> Result<(), String> {
        let mut project = self.load_project().await?;
        let mut found = false;
        for milestone in &mut project.roadmap.milestones {
            for phase in &mut milestone.phases {
                let len_before = phase.goals.len();
                phase.goals.retain(|g| g.goal_id != goal_id);
                if phase.goals.len() < len_before {
                    found = true;
                }
            }
        }
        if !found {
            return Err(format!("Goal {goal_id} not found"));
        }
        self.save_project(&project).await
    }

    /// 更新特定目标的完成状态
    pub async fn update_goal_status(&self, goal_id: &str, status: MilestoneStatus) -> Result<(), String> {
        let mut project = self.load_project().await?;
        let mut found = false;
        for milestone in &mut project.roadmap.milestones {
            for phase in &mut milestone.phases {
                for goal in &mut phase.goals {
                    if goal.goal_id == goal_id {
                        goal.status = status.clone();
                        found = true;
                    }
                }
            }
        }
        if !found {
            return Err(format!("Goal {} not found in roadmap", goal_id));
        }
        self.save_project(&project).await
    }

    pub async fn get_project_progress(&self) -> Result<f64, String> {
        let project = self.load_project().await?;
        let total_goals: usize = project.roadmap.milestones.iter()
            .flat_map(|m| &m.phases)
            .flat_map(|p| &p.goals)
            .count();
        
        if total_goals == 0 {
            return Ok(0.0);
        }

        let completed_goals: usize = project.roadmap.milestones.iter()
            .flat_map(|m| &m.phases)
            .flat_map(|p| &p.goals)
            .filter(|g| g.status == MilestoneStatus::Completed)
            .count();

        Ok(completed_goals as f64 / total_goals as f64)
    }

    pub async fn get_full_roadmap(&self) -> Result<Project, String> {
        self.load_project().await
    }

    pub async fn get_next_goal(&self) -> Result<Option<GoalRef>, String> {
        let project = self.load_project().await?;
        
        for milestone in &project.roadmap.milestones {
            if milestone.status == MilestoneStatus::Completed {
                continue;
            }
            
            for phase in &milestone.phases {
                let dependencies_met = phase.depends_on.iter().all(|dep_id| {
                    project.roadmap.milestones.iter()
                        .flat_map(|m| &m.phases)
                        .any(|p| p.id == *dep_id && p.goals.iter().all(|g| g.status == MilestoneStatus::Completed))
                });

                if !dependencies_met {
                    continue;
                }

                for goal in &phase.goals {
                    if goal.status == MilestoneStatus::Pending {
                        return Ok(Some(goal.clone()));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Retrieve formatted context about where a specific goal fits in the project roadmap.
    /// Returns a string describing project progress, milestone/phase placement, and surrounding goals.
    pub async fn get_goal_context(&self, goal_id: &str) -> Result<String, String> {
        let project = self.load_project().await?;
        let total_goals: usize = project.roadmap.milestones.iter()
            .flat_map(|m| &m.phases)
            .map(|p| p.goals.len())
            .sum();
        let completed_goals: usize = project.roadmap.milestones.iter()
            .flat_map(|m| &m.phases)
            .flat_map(|p| &p.goals)
            .filter(|g| g.status == MilestoneStatus::Completed)
            .count();

        let mut context = format!(
            "Project: {} ({})\nOverall Progress: {}/{} goals completed ({:.0}%)\n\n",
            project.name,
            project.description,
            completed_goals,
            total_goals,
            if total_goals > 0 { completed_goals as f64 / total_goals as f64 * 100.0 } else { 0.0 },
        );

        for milestone in &project.roadmap.milestones {
            let ms_progress = milestone.calculate_progress();
            context.push_str(&format!("Milestone: {} [{:.0}%]\n", milestone.name, ms_progress * 100.0));
            for phase in &milestone.phases {
                context.push_str(&format!("  Phase: {}\n", phase.name));
                for goal in &phase.goals {
                    let marker = if goal.goal_id == goal_id {
                        ">>> "
                    } else {
                        match goal.status {
                            MilestoneStatus::Completed => "  ✓ ",
                            MilestoneStatus::InProgress => "  ▶ ",
                            _ => "    ",
                        }
                    };
                    context.push_str(&format!("{}{} ({:?})\n", marker, goal.description, goal.status));
                }
            }
        }
        Ok(context)
    }

    /// Generate a project roadmap from a natural language description using LLM.
    pub async fn auto_plan(
        &self,
        llm: &dyn crate::LlmClient,
        description: &str,
    ) -> Result<Project, String> {
        let system_prompt = "You are a project planning expert. Given a project description, generate a structured roadmap with milestones, phases, and goals. Respond ONLY with valid JSON in the following format:
{
  \"milestones\": [
    {
      \"id\": \"m1\",
      \"name\": \"Milestone Name\",
      \"phases\": [
        {
          \"id\": \"p1\",
          \"name\": \"Phase Name\",
          \"goals\": [
            {
              \"id\": \"g1\",
              \"description\": \"Goal description\"
            }
          ]
        }
      ]
    }
  ]
}
Produce 3-5 milestones with 1-3 phases each and 2-5 goals per phase. Make goals concrete and actionable.";

        let response = llm.chat(system_prompt, description, true).await
            .map_err(|e| format!("LLM call failed: {e}"))?;
        
        let parsed: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| format!("Failed to parse LLM response as JSON: {e}. Raw: {response}"))?;
        
        let milestones_raw = parsed["milestones"].as_array()
            .ok_or_else(|| "LLM response missing 'milestones' array".to_string())?;
        
        let name = parsed["name"].as_str().unwrap_or("Auto-generated Project");
        let mut project = Project::new(self.project_id.clone(), name.to_string(), description.to_string());
        
        for (mi, ms_val) in milestones_raw.iter().enumerate() {
            let ms_id = ms_val["id"].as_str().unwrap_or(&format!("m{}", mi + 1)).to_string();
            let ms_name = ms_val["name"].as_str().unwrap_or(&format!("Milestone {}", mi + 1)).to_string();
            let ms_phases = ms_val["phases"].as_array().cloned().unwrap_or_default();
            
            let mut phases = Vec::new();
            for (pi, ph_val) in ms_phases.iter().enumerate() {
                let ph_id = ph_val["id"].as_str().unwrap_or(&format!("{}_p{}", ms_id, pi + 1)).to_string();
                let ph_name = ph_val["name"].as_str().unwrap_or(&format!("Phase {}", pi + 1)).to_string();
                let goals_raw = ph_val["goals"].as_array().cloned().unwrap_or_default();
                
                let mut goals = Vec::new();
                for (gi, g_val) in goals_raw.iter().enumerate() {
                    let g_id = g_val["id"].as_str().unwrap_or(&format!("{}_g{}", ph_id, gi + 1)).to_string();
                    let g_desc = g_val["description"].as_str().unwrap_or("Untitled goal").to_string();
                    goals.push(GoalRef {
                        goal_id: g_id,
                        description: g_desc,
                        status: MilestoneStatus::Pending,
                    });
                }
                
                phases.push(sisyphus_types::Phase {
                    id: ph_id,
                    name: ph_name,
                    depends_on: vec![],
                    goals,
                });
            }
            
            project.roadmap.milestones.push(Milestone {
                id: ms_id,
                name: ms_name,
                status: MilestoneStatus::InProgress,
                phases,
            });
        }
        
        self.save_project(&project).await?;
        Ok(project)
    }

    /// Analyze current roadmap state and produce a structured reflection report.
    /// Rule-based: checks progress health, stalled goals, milestone balance.
    pub async fn reflect(&self) -> Result<ReflectionReport, String> {
        let project = self.load_project().await?;

        let mut total_goals = 0usize;
        let mut completed_goals = 0usize;
        let mut all_goals: Vec<&GoalRef> = Vec::new();
        let mut assessments = Vec::new();

        for milestone in &project.roadmap.milestones {
            let mut ms_completed = 0usize;
            let mut ms_total = 0usize;

            for phase in &milestone.phases {
                for goal in &phase.goals {
                    ms_total += 1;
                    total_goals += 1;
                    all_goals.push(goal);
                    if goal.status == MilestoneStatus::Completed {
                        ms_completed += 1;
                        completed_goals += 1;
                    }
                }
            }

            let ms_progress = if ms_total > 0 { ms_completed as f64 / ms_total as f64 } else { 1.0 };
            assessments.push(MilestoneAssessment {
                name: milestone.name.clone(),
                progress_pct: ms_progress * 100.0,
                status: format!("{:?}", milestone.status),
                goal_count: ms_total,
                completed_count: ms_completed,
                is_on_track: ms_progress >= 0.3 || milestone.status == sisyphus_types::MilestoneStatus::Completed,
            });
        }

        let overall_pct = if total_goals > 0 { completed_goals as f64 / total_goals as f64 * 100.0 } else { 0.0 };
        let is_on_track = completed_goals > 0 || total_goals == 0;

        let mut stalled_goals = Vec::new();
        let mut recently_completed = Vec::new();
        for goal in &all_goals {
            if goal.status == MilestoneStatus::Pending && stalled_goals.len() < 3 {
                stalled_goals.push(goal.description.clone());
            } else if goal.status == MilestoneStatus::Completed && recently_completed.len() < 3 {
                recently_completed.push(goal.description.clone());
            }
        }

        let mut recommendations = Vec::new();
        if overall_pct < 20.0 && total_goals > 0 {
            recommendations.push("Project is in early stage. Focus on completing foundational milestones.".to_string());
        }

        let stalled_ms: Vec<&str> = assessments.iter()
            .filter(|a| a.progress_pct < 10.0 && a.status != "Completed")
            .map(|a| a.name.as_str())
            .collect();
        if !stalled_ms.is_empty() {
            recommendations.push(format!(
                "Milestone(s) with minimal progress: {}. Consider re-scoping or prioritizing.",
                stalled_ms.join(", ")
            ));
        }

        if overall_pct > 80.0 && overall_pct < 100.0 {
            recommendations.push("Project is near completion. Drive remaining goals to close out.".to_string());
        }

        if assessments.len() >= 2 {
            let max_progress = assessments.iter().map(|a| a.progress_pct).fold(0.0_f64, f64::max);
            let min_progress = assessments.iter().map(|a| a.progress_pct).fold(100.0_f64, f64::min);
            if max_progress - min_progress > 50.0 {
                recommendations.push(format!(
                    "Significant progress imbalance detected ({:.0}% vs {:.0}%). Re-balance effort across milestones.",
                    max_progress, min_progress,
                ));
            }
        }

        Ok(ReflectionReport {
            project_id: self.project_id.clone(),
            project_name: project.name.clone(),
            overall_progress_pct: overall_pct,
            milestones: assessments,
            is_on_track,
            stalled_goals,
            completed_recently: recently_completed,
            recommendations,
        })
    }

    /// Predict milestone completion based on current progress.
    /// Uses goal completion ratios and milestone status to estimate risk and ordering.
    pub async fn predict_completion(&self) -> Result<ProgressPrediction, String> {
        let project = self.load_project().await?;

        let mut total_goals = 0usize;
        let mut completed_goals = 0usize;
        let mut milestone_preds = Vec::new();
        let mut risk_levels = Vec::new();

        for milestone in &project.roadmap.milestones {
            let mut ms_total = 0usize;
            let mut ms_completed = 0usize;

            for phase in &milestone.phases {
                for goal in &phase.goals {
                    ms_total += 1;
                    total_goals += 1;
                    if goal.status == MilestoneStatus::Completed {
                        ms_completed += 1;
                        completed_goals += 1;
                    }
                }
            }

            let progress = if ms_total > 0 { ms_completed as f64 / ms_total as f64 } else { 1.0 };
            let remaining = ms_total.saturating_sub(ms_completed);

            let risk = match milestone.status {
                sisyphus_types::MilestoneStatus::Completed => "done",
                sisyphus_types::MilestoneStatus::InProgress if progress > 0.5 => "low",
                sisyphus_types::MilestoneStatus::InProgress => "medium",
                _ => "high",
            };
            risk_levels.push(risk);

            milestone_preds.push(MilestonePrediction {
                name: milestone.name.clone(),
                progress_pct: progress * 100.0,
                remaining_goals: remaining,
                risk_level: risk.to_string(),
            });
        }

        let overall_pct = if total_goals > 0 { completed_goals as f64 / total_goals as f64 * 100.0 } else { 0.0 };
        let remaining_goals = total_goals.saturating_sub(completed_goals);

            // highest progress first
            let mut sorted: Vec<&MilestonePrediction> = milestone_preds.iter()
            .filter(|m| m.risk_level != "done")
            .collect();
        sorted.sort_by(|a, b| b.progress_pct.partial_cmp(&a.progress_pct).unwrap_or(std::cmp::Ordering::Equal));
        let completion_order: Vec<String> = sorted.iter().map(|m| m.name.clone()).collect();

        let overall_risk = if risk_levels.contains(&"high") { "high"
            } else if risk_levels.contains(&"medium") { "medium"
            } else { "low" }.to_string();

        Ok(ProgressPrediction {
            project_id: self.project_id.clone(),
            project_name: project.name.clone(),
            overall_progress_pct: overall_pct,
            remaining_goals,
            milestone_predictions: milestone_preds,
            estimated_completion_order: completion_order,
            risk_level: overall_risk,
        })
    }

}

/// Structured reflection report analyzing roadmap health.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReflectionReport {
    pub project_id: String,
    pub project_name: String,
    pub overall_progress_pct: f64,
    pub milestones: Vec<MilestoneAssessment>,
    pub is_on_track: bool,
    pub stalled_goals: Vec<String>,
    pub completed_recently: Vec<String>,
    pub recommendations: Vec<String>,
}

/// Per-milestone assessment within a reflection report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MilestoneAssessment {
    pub name: String,
    pub progress_pct: f64,
    pub status: String,
    pub goal_count: usize,
    pub completed_count: usize,
    pub is_on_track: bool,
}

/// Predicted completion timeline for a project based on current progress.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgressPrediction {
    pub project_id: String,
    pub project_name: String,
    pub overall_progress_pct: f64,
    pub remaining_goals: usize,
    pub milestone_predictions: Vec<MilestonePrediction>,
    pub estimated_completion_order: Vec<String>,
    pub risk_level: String,
}

/// Per-milestone prediction within a progress forecast.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MilestonePrediction {
    pub name: String,
    pub progress_pct: f64,
    pub remaining_goals: usize,
    pub risk_level: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::Phase;
    use std::collections::HashMap;

    struct MockStore {
        data: std::sync::Mutex<HashMap<String, serde_json::Value>>,
    }

    impl MockStore {
        fn new() -> Self {
            Self { data: std::sync::Mutex::new(HashMap::new()) }
        }
    }

    #[async_trait::async_trait]
    impl StateStore for MockStore {
        async fn get_state_json(&self, key: &str) -> Result<Option<serde_json::Value>, String> {
            Ok(self.data.lock().unwrap().get(key).cloned())
        }
        async fn set_state_json(&self, key: &str, value: serde_json::Value) -> Result<(), String> {
            self.data.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }
    }

    fn make_milestone(id: &str, name: &str, goals: Vec<(&str, &str, MilestoneStatus)>) -> Milestone {
        Milestone {
            id: id.into(),
            name: name.into(),
            status: MilestoneStatus::InProgress,
            phases: vec![Phase {
                id: format!("{}_phase", id),
                name: format!("{} Phase", name),
                depends_on: vec![],
                goals: goals.into_iter().map(|(gid, desc, status)| GoalRef {
                    goal_id: gid.into(),
                    description: desc.into(),
                    status,
                }).collect(),
            }],
        }
    }

    // ── Milestone.calculate_progress ────────────────────────────────

    #[test]
    fn test_milestone_progress_empty() {
        let m = make_milestone("m0", "Empty", vec![]);
        assert_eq!(m.calculate_progress(), 0.0);
    }

    #[test]
    fn test_milestone_progress_half() {
        let m = make_milestone("m1", "Half", vec![
            ("g1", "Goal 1", MilestoneStatus::Completed),
            ("g2", "Goal 2", MilestoneStatus::Pending),
        ]);
        assert!((m.calculate_progress() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_milestone_progress_all_done() {
        let m = make_milestone("m2", "All Done", vec![
            ("g1", "Goal 1", MilestoneStatus::Completed),
            ("g2", "Goal 2", MilestoneStatus::Completed),
        ]);
        assert!((m.calculate_progress() - 1.0).abs() < f64::EPSILON);
    }

    // ── RoadmapManager lifecycle ────────────────────────────────────

    #[tokio::test]
    async fn test_roadmap_lifecycle() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_1".into());

        // 1. Create project
        let proj = manager.create_project("Test Project".into(), "A test project description".into()).await.unwrap();
        assert_eq!(proj.name, "Test Project");

        // 2. Add milestone
        let milestone = make_milestone("m1", "Phase 1", vec![
            ("g1", "Goal 1", MilestoneStatus::Pending),
        ]);
        manager.add_milestone(milestone).await.unwrap();

        // 3. Get next goal
        let next = manager.get_next_goal().await.unwrap();
        assert!(next.is_some());
        assert_eq!(next.unwrap().goal_id, "g1");

        // 4. Update status
        manager.update_goal_status("g1", MilestoneStatus::Completed).await.unwrap();
        let next_after = manager.get_next_goal().await.unwrap();
        assert!(next_after.is_none());
    }

    #[tokio::test]
    async fn test_add_goal_to_phase() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_2".into());

        manager.create_project("P2".into(), "desc".into()).await.unwrap();
        manager.add_milestone(make_milestone("m1", "M1", vec![
            ("g1", "Original", MilestoneStatus::Pending),
        ])).await.unwrap();

        manager.add_goal_to_phase("m1", "m1_phase", GoalRef {
            goal_id: "g2".into(),
            description: "Added".into(),
            status: MilestoneStatus::Pending,
        }).await.unwrap();

        let g1 = manager.get_next_goal().await.unwrap().unwrap();
        assert_eq!(g1.goal_id, "g1");
        manager.update_goal_status("g1", MilestoneStatus::Completed).await.unwrap();
        let g2 = manager.get_next_goal().await.unwrap().unwrap();
        assert_eq!(g2.goal_id, "g2");
    }

    #[tokio::test]
    async fn test_remove_goal() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_3".into());

        manager.create_project("P3".into(), "desc".into()).await.unwrap();
        manager.add_milestone(make_milestone("m1", "M1", vec![
            ("g1", "To remove", MilestoneStatus::Pending),
            ("g2", "Keep", MilestoneStatus::Pending),
        ])).await.unwrap();

        manager.remove_goal("g1").await.unwrap();
        let next = manager.get_next_goal().await.unwrap().unwrap();
        assert_eq!(next.goal_id, "g2");
    }

    #[tokio::test]
    async fn test_remove_goal_not_found() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_4".into());
        manager.create_project("P4".into(), "desc".into()).await.unwrap();

        let err = manager.remove_goal("nonexistent").await.unwrap_err();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_project_progress() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_5".into());

        manager.create_project("P5".into(), "desc".into()).await.unwrap();
        manager.add_milestone(make_milestone("m1", "M1", vec![
            ("g1", "Goal 1", MilestoneStatus::Completed),
            ("g2", "Goal 2", MilestoneStatus::Pending),
            ("g3", "Goal 3", MilestoneStatus::Completed),
        ])).await.unwrap();

        let progress = manager.get_project_progress().await.unwrap();
        assert!((progress - 2.0 / 3.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_project_progress_empty() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_6".into());
        manager.create_project("P6".into(), "desc".into()).await.unwrap();

        let progress = manager.get_project_progress().await.unwrap();
        assert_eq!(progress, 0.0);
    }

    #[tokio::test]
    async fn test_project_not_found() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "nonexistent".into());

        let err = manager.load_project().await.unwrap_err();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_update_goal_status_not_found() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_7".into());
        manager.create_project("P7".into(), "desc".into()).await.unwrap();

        let err = manager.update_goal_status("ghost", MilestoneStatus::Completed).await.unwrap_err();
        assert!(err.contains("not found in roadmap"));
    }

    #[tokio::test]
    async fn test_get_next_goal_multiple_milestones() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_8".into());

        manager.create_project("P8".into(), "desc".into()).await.unwrap();
        manager.add_milestone(make_milestone("m1", "First", vec![
            ("g1", "First goal", MilestoneStatus::Pending),
        ])).await.unwrap();
        manager.add_milestone(make_milestone("m2", "Second", vec![
            ("g2", "Second goal", MilestoneStatus::Pending),
        ])).await.unwrap();

        let next = manager.get_next_goal().await.unwrap().unwrap();
        assert_eq!(next.goal_id, "g1");
    }

    #[tokio::test]
    async fn test_get_next_goal_skips_completed_milestone() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_9".into());

        manager.create_project("P9".into(), "desc".into()).await.unwrap();

        // Completed milestone with incomplete goals should be skipped
        let mut ms = make_milestone("m_done", "Done", vec![
            ("g_done", "Will be done", MilestoneStatus::Pending),
        ]);
        ms.status = MilestoneStatus::Completed;
        manager.add_milestone(ms).await.unwrap();

        manager.add_milestone(make_milestone("m_next", "Next", vec![
            ("g_next", "Next goal", MilestoneStatus::Pending),
        ])).await.unwrap();

        let next = manager.get_next_goal().await.unwrap().unwrap();
        assert_eq!(next.goal_id, "g_next");
    }

    #[tokio::test]
    async fn test_get_full_roadmap_roundtrip() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_10".into());

        manager.create_project("P10".into(), "desc".into()).await.unwrap();
        manager.add_milestone(make_milestone("m1", "M1", vec![
            ("g1", "G1", MilestoneStatus::Pending),
        ])).await.unwrap();

        let project = manager.get_full_roadmap().await.unwrap();
        assert_eq!(project.name, "P10");
        assert_eq!(project.roadmap.milestones.len(), 1);
        assert_eq!(project.roadmap.milestones[0].phases[0].goals[0].goal_id, "g1");
    }

    #[tokio::test]
    async fn test_save_project_overwrites() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_11".into());

        let mut project = manager.create_project("Original".into(), "desc".into()).await.unwrap();
        project.name = "Overwritten".into();
        let saved = manager.save_project(&project).await;
        assert!(saved.is_ok());

        let loaded = manager.load_project().await.unwrap();
        assert_eq!(loaded.name, "Overwritten");
    }

    #[tokio::test]
    async fn test_auto_plan_parses_llm_response() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_auto".into());

        let llm_mock = crate::FixedResponseClient(r#"{
            "name": "Auto Test Project",
            "milestones": [
                {
                    "id": "m1",
                    "name": "Foundation",
                    "phases": [
                        {
                            "id": "p1",
                            "name": "Setup",
                            "goals": [
                                {"id": "g1", "description": "Install tools"},
                                {"id": "g2", "description": "Configure environment"}
                            ]
                        }
                    ]
                },
                {
                    "id": "m2",
                    "name": "Core Features",
                    "phases": [
                        {
                            "id": "p2",
                            "name": "Backend",
                            "goals": [
                                {"id": "g3", "description": "Build API"}
                            ]
                        }
                    ]
                }
            ]
        }"#.to_string());

        let project = manager.auto_plan(&llm_mock, "Test project description").await.unwrap();
        assert_eq!(project.name, "Auto Test Project");
        assert_eq!(project.roadmap.milestones.len(), 2);
        assert_eq!(project.roadmap.milestones[0].phases[0].goals.len(), 2);
        assert_eq!(project.roadmap.milestones[1].phases[0].goals[0].goal_id, "g3");
        assert_eq!(project.roadmap.milestones[0].name, "Foundation");
    }

    #[tokio::test]
    async fn test_auto_plan_invalid_json() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_bad".into());

        let llm_mock = crate::FixedResponseClient("not valid json".to_string());
        let err = manager.auto_plan(&llm_mock, "Bad project").await.unwrap_err();
        assert!(err.contains("Failed to parse"));
    }

    #[tokio::test]
    async fn test_goal_context_contains_progress() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_ctx".into());

        manager.create_project("CtxTest".into(), "Context test".into()).await.unwrap();
        manager.add_milestone(make_milestone("m1", "Phase 1", vec![
            ("g1", "First goal", MilestoneStatus::Completed),
            ("g2", "Second goal", MilestoneStatus::Pending),
        ])).await.unwrap();

        let context = manager.get_goal_context("g2").await.unwrap();
        assert!(context.contains("CtxTest"));
        assert!(context.contains("50%") || context.contains("1/2")); // 1/2 completed
        assert!(context.contains(">>>")); // g2 is the current goal
        assert!(context.contains("Second goal"));
        assert!(context.contains("First goal"));
    }

    #[tokio::test]
    async fn test_goal_context_not_found() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_ctx2".into());

        let err = manager.get_goal_context("ghost").await.unwrap_err();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_reflect_empty_project() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_re0".into());
        manager.create_project("ReflectTest".into(), "".into()).await.unwrap();

        let report = manager.reflect().await.unwrap();
        assert!(report.is_on_track);
        assert_eq!(report.overall_progress_pct, 0.0);
        assert!(report.milestones.is_empty());
    }

    #[tokio::test]
    async fn test_reflect_with_progress() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_re1".into());
        manager.create_project("ReflectProgress".into(), "Test".into()).await.unwrap();
        manager.add_milestone(make_milestone("ref_m1", "Milestone A", vec![
            ("g_a1", "Goal A1", MilestoneStatus::Completed),
            ("g_a2", "Goal A2", MilestoneStatus::Pending),
        ])).await.unwrap();
        manager.add_milestone(make_milestone("ref_m2", "Milestone B", vec![
            ("g_b1", "Goal B1", MilestoneStatus::Pending),
        ])).await.unwrap();

        let report = manager.reflect().await.unwrap();
        assert_eq!(report.project_name, "ReflectProgress");
        assert!((report.overall_progress_pct - 33.33).abs() < 1.0);
        assert_eq!(report.milestones.len(), 2);
        assert!(report.milestones[0].is_on_track);
        assert!(!report.milestones[1].is_on_track);
        let has_stalled_rec = report.recommendations.iter()
            .any(|r| r.contains("Milestone B"));
        assert!(has_stalled_rec, "Should recommend focusing on stalled milestone");
    }

    #[tokio::test]
    async fn test_reflect_near_completion() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_re2".into());
        manager.create_project("NearDone".into(), "".into()).await.unwrap();
        manager.add_milestone(make_milestone("rm1", "M1", vec![
            ("g1", "G1", MilestoneStatus::Completed),
            ("g2", "G2", MilestoneStatus::Completed),
            ("g3", "G3", MilestoneStatus::Pending),
        ])).await.unwrap();

        let report = manager.reflect().await.unwrap();
        assert!((report.overall_progress_pct - 66.67).abs() < 1.0);
        assert!(report.is_on_track);
    }

    #[tokio::test]
    async fn test_predict_empty_project() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_pr0".into());
        manager.create_project("EmptyPred".into(), "".into()).await.unwrap();

        let pred = manager.predict_completion().await.unwrap();
        assert_eq!(pred.remaining_goals, 0);
        assert_eq!(pred.risk_level, "low");
        assert!(pred.estimated_completion_order.is_empty());
    }

    #[tokio::test]
    async fn test_predict_high_risk_pending() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_pr1".into());
        manager.create_project("HighRisk".into(), "".into()).await.unwrap();
        manager.add_milestone(make_milestone("pm1", "Future Work", vec![
            ("pg1", "Not started", MilestoneStatus::Pending),
        ])).await.unwrap();

        let pred = manager.predict_completion().await.unwrap();
        assert_eq!(pred.remaining_goals, 1);
        // milestone is InProgress with 0% progress → "medium"
        assert_eq!(pred.risk_level, "medium");
        assert_eq!(pred.milestone_predictions.len(), 1);
        assert_eq!(pred.milestone_predictions[0].risk_level, "medium");
    }

    #[tokio::test]
    async fn test_predict_mixed_risk() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_pr2".into());
        manager.create_project("MixedRisk".into(), "".into()).await.unwrap();
        manager.add_milestone(make_milestone("pm_a", "Done MS", vec![
            ("pga1", "G1", MilestoneStatus::Completed),
        ])).await.unwrap();
        manager.add_milestone(make_milestone("pm_b", "In Prog MS", vec![
            ("pgb1", "G1", MilestoneStatus::Completed),
            ("pgb2", "G2", MilestoneStatus::Pending),
        ])).await.unwrap();
        manager.add_milestone(make_milestone("pm_c", "Not Started", vec![
            ("pgc1", "G1", MilestoneStatus::Pending),
        ])).await.unwrap();

        let pred = manager.predict_completion().await.unwrap();
        // All milestones are InProgress. Risk = progress-based:
        //   Done MS: 100% → low
        //   In Prog MS: 50% → medium (= 0.5, not > 0.5)
        //   Not Started: 0% → medium
        // Overall: medium (highest)
        assert_eq!(pred.risk_level, "medium");
        assert_eq!(pred.milestone_predictions.len(), 3);
        assert_eq!(pred.milestone_predictions[0].risk_level, "low");
        assert_eq!(pred.milestone_predictions[1].risk_level, "medium");
        assert_eq!(pred.milestone_predictions[2].risk_level, "medium");
        assert!((pred.overall_progress_pct - 50.0).abs() < 1.0);
        // Completion order: Done MS first (100%), then In Prog MS (50%), then Not Started (0%)
        assert_eq!(pred.estimated_completion_order, vec!["Done MS", "In Prog MS", "Not Started"]);
    }

    #[tokio::test]
    async fn test_strategic_pipeline_end_to_end() {
        let store = Arc::new(MockStore::new());
        let manager = RoadmapManager::new(store, "proj_e2e".into());

        manager.create_project("E2E Project".into(), "Integration test".into()).await.unwrap();

        let llm_mock = crate::FixedResponseClient(r#"{
            "name": "E2E Project",
            "milestones": [
                {
                    "id": "e2e_m1",
                    "name": "Foundation",
                    "phases": [{
                        "id": "e2e_p1",
                        "name": "Setup",
                        "goals": [
                            {"id": "e2e_g1", "description": "Install tools"},
                            {"id": "e2e_g2", "description": "Configure env"}
                        ]
                    }]
                },
                {
                    "id": "e2e_m2",
                    "name": "Core",
                    "phases": [{
                        "id": "e2e_p2",
                        "name": "Backend",
                        "goals": [
                            {"id": "e2e_g3", "description": "Build API"}
                        ]
                    }]
                }
            ]
        }"#.to_string());

        let project = manager.auto_plan(&llm_mock, "E2E test project").await.unwrap();
        assert_eq!(project.roadmap.milestones.len(), 2);

        manager.update_goal_status("e2e_g1", MilestoneStatus::Completed).await.unwrap();

        let progress = manager.get_project_progress().await.unwrap();
        assert!((progress * 100.0 - 33.33).abs() < 1.0);

        let next = manager.get_next_goal().await.unwrap().unwrap();
        assert_eq!(next.goal_id, "e2e_g2");

        let ctx = manager.get_goal_context("e2e_g2").await.unwrap();
        assert!(ctx.contains("Foundation"));
        assert!(ctx.contains(">>>"));

        let report = manager.reflect().await.unwrap();
        assert_eq!(report.milestones.len(), 2);

        let pred = manager.predict_completion().await.unwrap();
        assert_eq!(pred.remaining_goals, 2);
        assert_eq!(pred.risk_level, "medium");
    }
}

