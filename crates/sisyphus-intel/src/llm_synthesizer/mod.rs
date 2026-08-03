//! LLM-driven and logical SOP synthesizers.
//!
//! Provides [`GoalDecomposer`] implementations for action decomposition and the
//! [`FlowSynthesizer`] implementations that turn goals into SOP graphs. The
//! module is split into domain files:
//! - [`decomposers`]: goal decomposition strategies
//! - [`synthesizer`]: the LLM-driven pipeline synthesizer
//! - [`logical`]: the adversarial, evaluator-driven logical synthesizer

mod decomposers;
mod logical;
mod synthesizer;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicLesson {
    pub goal: String,
    pub sop_json: String,
    pub feedback: String,
    pub success: bool,
}

pub use decomposers::{FallbackGoalDecomposer, GoalDecomposer, LlmGoalDecomposer};
pub use logical::LogicalSOPSynthesizer;
pub use synthesizer::LlmDrivenSynthesizer;
