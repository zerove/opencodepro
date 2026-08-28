#![allow(clippy::cast_precision_loss)]
pub mod complexity;
pub mod gap_analyzer;
pub mod pattern_analyzer;
pub mod tool_coder;
pub mod tool_verifier;
pub mod wasm_compiler;
use std::sync::Arc;

pub use sisyphus_types::errors::IntelError;
pub mod deep_reflector;
pub mod evaluator;
pub mod failure_pattern_evolver;
pub mod formal_verification;
pub mod fuel;
pub mod llm_synthesizer;
pub mod micro_reflector;
pub mod mock;
pub mod probabilistic_planner;
pub mod reflect;
pub mod self_programming;
pub mod simulation;
pub mod simulation_gate;
pub mod supervisor;
pub mod synthesis;
pub mod token_budget;
pub mod topological_evolver;

pub use complexity::{CognitiveDepth, ComplexityAnalyzer};
pub use fuel::{AdaptiveFuelPricer, FuelPricer, SimpleFuelPricer};
pub use gap_analyzer::{GapAnalyzer, ToolGap};
pub use reflect::{
    ErrorCategory, HybridReflector, LlmEvolver, LlmReflector, RecordedNode, ReflectionReport,
    Reflector, SOPEvolver, StandardEvolver, StandardReflector,
};
pub use simulation::{MonteCarloSimulator, SimulationResult, Simulator};
pub use supervisor::SisyphusSupervisor;
pub(crate) use synthesis::{action_label, classify_goal, tool_id_for_action};
pub use synthesis::{
    synthesize_composite, ActionType, FlowSynthesizer, GoalDrivenSynthesizer,
    MetaLearningSynthesizer, StrategicSopSynthesizer, SynthesisResult,
};
pub use tool_coder::ToolCoder;
pub use tool_verifier::{ToolVerifier, VerificationReport};
pub use wasm_compiler::WasmCompiler;

pub use deep_reflector::{
    CognitiveCorrection, CorrectionPriority, DeepDeviationReport, DeepReflector,
};
pub use failure_pattern_evolver::{
    FailureCategory, FailurePattern, FailurePatternEvolver, FailurePatternLibrary,
    MutationSuggestion,
};
pub use formal_verification::{VerificationReport as FormalVerificationReport, VerificationRunner};
pub use llm_synthesizer::{
    FallbackGoalDecomposer, GoalDecomposer, LlmDrivenSynthesizer, LlmGoalDecomposer,
    LogicalSOPSynthesizer,
};
pub use probabilistic_planner::{PathSummary, PlannerConfig, ProbabilisticPlanner, RankedPath};
pub use topological_evolver::TopologicalSOPEvolver;

// ---------------------------------------------------------------------------
// 情报编排器
// ---------------------------------------------------------------------------

pub struct SisyphusIntel {
    pub simulator: Arc<dyn Simulator>,
    pub logical_simulator: Arc<dyn simulation::LogicalSimulator>,
    pub reflector: Arc<dyn Reflector>,
    pub evolver: Arc<dyn SOPEvolver>,
}

impl SisyphusIntel {
    pub fn new(
        simulator: Arc<dyn Simulator>,
        logical_simulator: Arc<dyn simulation::LogicalSimulator>,
        reflector: Arc<dyn Reflector>,
        evolver: Arc<dyn SOPEvolver>,
    ) -> Self {
        Self {
            simulator,
            logical_simulator,
            reflector,
            evolver,
        }
    }
}
