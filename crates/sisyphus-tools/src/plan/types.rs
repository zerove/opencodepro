use serde::{Deserialize, Serialize};

/// Status of a plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlanStatus {
    Draft,
    Reviewed,
    Approved,
    Cancelled,
}

impl std::fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanStatus::Draft => write!(f, "draft"),
            PlanStatus::Reviewed => write!(f, "reviewed"),
            PlanStatus::Approved => write!(f, "approved"),
            PlanStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A single step in a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub description: String,
    pub status: PlanStatus,
    pub dependencies: Vec<String>,
    pub estimated_effort: Option<String>,
}

/// A structured plan with multiple steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: PlanStatus,
    pub steps: Vec<PlanStep>,
    pub created_at: String,
    pub updated_at: String,
}
