//! Cognitive Benchmark Suite
//!
//! This test suite evaluates the convergence and success rate of the Sisyphus cognitive loop
//! using a set of high-complexity architectural goals.

use async_trait::async_trait;
use sisyphus_core::{CoreError, SisyphusKernel};
use sisyphus_intel::synthesis::ActionType;
use sisyphus_intel::{
    GoalDecomposer, LlmDrivenSynthesizer, MonteCarloSimulator, Reflector, SOPEvolver, Simulator,
    SisyphusIntel, StandardEvolver, StandardReflector,
};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::metis::DummyLlmClient;
use sisyphus_types::{ExecutionContext, SOPGraph, TypesError};
use std::sync::Arc;
use tokio::sync::mpsc;

// --- Mock Implementations ---

struct MockConsensus;
#[async_trait]
impl sisyphus_core::Consensus for MockConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "mock-leader".into()
    }
    async fn propose(&self, _key: &str, _value: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
}

struct MockGoalDecomposer;
#[async_trait]
impl GoalDecomposer for MockGoalDecomposer {
    async fn decompose(&self, _goal: &str) -> Result<Vec<ActionType>, String> {
        Ok(vec![
            ActionType::Fetch,
            ActionType::Process,
            ActionType::Validate,
        ])
    }
}

struct MockExecutor;
#[async_trait]
impl sisyphus_types::ToolExecutor for MockExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        Ok(b"Mock output".to_vec())
    }
}

// --- Benchmark Definitions ---

struct BenchmarkGoal {
    id: &'static str,
    description: &'static str,
}

const HARD_GOALS: &[BenchmarkGoal] = &[
    BenchmarkGoal {
        id: "GOAL_DEP_REFACTOR",
        description: "Refactor the StateStore to use a distributed consensus mechanism without breaking existing APIs.",
    },
    BenchmarkGoal {
        id: "GOAL_PERF_OPTIMIZE",
        description: "Analyze the SOPGraph cache mechanism and introduce a multi-level caching strategy to reduce subgraph parsing latency by 50%.",
    },
    BenchmarkGoal {
        id: "GOAL_SECURITY_ACL",
        description: "Implement a permission-based Access Control List (ACL) for all ToolExecutors to prevent unauthorized use of sensitive tools.",
    },
];

async fn setup_test_kernel() -> SisyphusKernel {
    let store = Arc::new(
        MfsStore::new("sqlite::memory:")
            .await
            .expect("Store init failed"),
    );
    let tool_executor = Arc::new(MockExecutor);
    let llm_client = Arc::new(DummyLlmClient);

    let simulator = Arc::new(MonteCarloSimulator::new(100));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));

    let synthesizer = Arc::new(LlmDrivenSynthesizer::new(
        Arc::new(MockGoalDecomposer),
        tool_executor.clone(),
    ));

    SisyphusKernel::new(
        store,
        tool_executor,
        intel,
        llm_client,
        synthesizer,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
    )
}

#[tokio::test]
async fn test_cognitive_convergence_benchmark() {
    let kernel = setup_test_kernel().await;
    let mut overall_success = 0;

    println!("\n=== Sisyphus Cognitive Benchmark Suite ===");
    println!(
        "{:<20} | {:<10} | {:<10} | {:<10}",
        "Goal ID", "Iter", "Dev Score", "Status"
    );
    println!("{:-<60}", "");

    for goal in HARD_GOALS {
        let sop_id = format!("bench_{}", goal.id);

        let ctx = ExecutionContext::new();
        let synthesis = kernel
            .intelligence_manager
            .synthesize_goal(
                &goal.description,
                &ctx,
                sisyphus_types::WorkMode::UltraWork,
                None,
            )
            .await
            .expect("Synthesis failed");

        let graph = synthesis.graph;

        let result = kernel
            .execute_with_cognition(graph, &sop_id, None, None, None)
            .await;

        match result {
            Ok((_output, report, _evolved)) => {
                let status = if report.converged {
                    "CONVERGED"
                } else {
                    "FAILED"
                };
                if report.converged {
                    overall_success += 1;
                }

                println!(
                    "{:<20} | {:<10} | {:<10.4} | {:<10}",
                    goal.id, report.iterations, report.final_deviation, status
                );
            }
            Err(e) => {
                println!(
                    "{:<20} | {:<10} | {:<10} | {:<10}",
                    goal.id, "ERR", "N/A", "ERROR"
                );
                tracing::error!("Benchmark goal {} failed: {}", goal.id, e);
            }
        }
    }

    println!("{:-<60}", "");
    println!(
        "Overall Success Rate: {:.2}%",
        (overall_success as f32 / HARD_GOALS.len() as f32) * 100.0
    );

    assert!(
        overall_success > 0,
        "The system failed to converge on any hard goals"
    );
}
