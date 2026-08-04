//! 路线图错误类型。

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RoadmapError {
    #[error("Project '{0}' not found")]
    ProjectNotFound(String),
    #[error("Milestone '{0}' not found in project '{1}'")]
    MilestoneNotFound(String, String),
    #[error("Phase '{0}' not found in milestone '{1}'")]
    PhaseNotFound(String, String),
    #[error("Goal '{0}' not found in phase '{1}'")]
    GoalNotFound(String, String),
    #[error("Duplicate project id: '{0}' already exists")]
    DuplicateProject(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Persistence error: {0}")]
    Persistence(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<serde_json::Error> for RoadmapError {
    fn from(e: serde_json::Error) -> Self {
        RoadmapError::Serialization(e.to_string())
    }
}

impl From<sisyphus_types::TypesError> for RoadmapError {
    fn from(e: sisyphus_types::TypesError) -> Self {
        RoadmapError::Persistence(e.to_string())
    }
}

impl From<String> for RoadmapError {
    fn from(e: String) -> Self {
        RoadmapError::Persistence(e)
    }
}
