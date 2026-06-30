use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum MilestoneStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GoalRef {
    pub goal_id: String,
    pub description: String,
    pub status: MilestoneStatus,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Phase {
    pub id: String,
    pub name: String,
    pub goals: Vec<GoalRef>,
    pub depends_on: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Milestone {
    pub id: String,
    pub name: String,
    pub phases: Vec<Phase>,
    pub status: MilestoneStatus,
}

impl Milestone {
    pub fn calculate_progress(&self) -> f64 {
        let total_goals: usize = self.phases.iter().map(|p| p.goals.len()).sum();
        if total_goals == 0 {
            return 0.0;
        }
        let completed_goals: usize = self
            .phases
            .iter()
            .flat_map(|p| &p.goals)
            .filter(|g| g.status == MilestoneStatus::Completed)
            .count();
        completed_goals as f64 / total_goals as f64
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Roadmap {
    pub milestones: Vec<Milestone>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub roadmap: Roadmap,
}

impl Project {
    pub fn new(id: String, name: String, description: String) -> Self {
        Self {
            id,
            name,
            description,
            roadmap: Roadmap {
                milestones: Vec::new(),
            },
        }
    }
}
