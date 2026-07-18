#![allow(clippy::cast_precision_loss)]
pub mod milestone_generator;
pub mod proposal_manager;
pub mod resource_optimizer;
pub mod strategic_reflection;

pub mod error;
pub use error::RoadmapError;

pub mod types;
pub use types::*;

mod roadmap_manager;
pub use roadmap_manager::RoadmapManager;
mod roadmap_health;
mod roadmap_prediction;
mod roadmap_progress;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
