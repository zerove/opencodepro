#![allow(clippy::cast_precision_loss)]
use std::sync::Arc;

pub mod error;
pub mod formal_verification;
pub mod fuel;
pub mod llm_synthesizer;
pub mod mock;
pub mod reflect;
pub mod simulation;
pub mod synthesis;
pub mod topological_evolver;

pub use error::IntelError;
pub use fuel::{AdaptiveFuelPricer, FuelPricer, SimpleFuelPricer};
pub use reflect::{
    ErrorCategory, RecordedNode, ReflectionReport, Reflector, SOPEvolver, StandardEvolver,
    StandardReflector,
};
pub use simulation::{MonteCarloSimulator, Simulator, SimulationResult};
pub(crate) use synthesis::{
    action_label, classify_goal, tool_id_for_action,
};
pub use synthesis::{
    synthesize_composite, ActionType, FlowSynthesizer, GoalDrivenSynthesizer, SynthesisResult,
};

pub use formal_verification::{VerificationReport, VerificationRunner};
pub use llm_synthesizer::{
    FallbackGoalDecomposer, GoalDecomposer, LlmDrivenSynthesizer, LlmGoalDecomposer,
};
pub use topological_evolver::TopologicalSOPEvolver;

// ---------------------------------------------------------------------------
// Intel orchestrator
// ---------------------------------------------------------------------------

pub struct SisyphusIntel {
    pub simulator: Arc<dyn Simulator>,
    pub reflector: Arc<dyn Reflector>,
    pub evolver: Arc<dyn SOPEvolver>,
}

impl SisyphusIntel {
    pub fn new(
        simulator: Arc<dyn Simulator>,
        reflector: Arc<dyn Reflector>,
        evolver: Arc<dyn SOPEvolver>,
    ) -> Self {
        Self {
            simulator,
            reflector,
            evolver,
        }
    }
}
