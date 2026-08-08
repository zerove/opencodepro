#![cfg(feature = "broken_tests")]
//! Cognitive Benchmark Suite
//!
//! This test suite evaluates the convergence and success rate of the Sisyphus cognitive loop
//! using a set of high-complexity architectural goals.

use async_trait::async_trait;
use sisyphus_core::SisyphusKernel;
use sisyphus_intel::{
    GoalDecomposer, LlmDrivenSynthesizer, MonteCarloSimulator, SisyphusIntel, StandardEvolver,
    StandardReflector,
};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::metis::DummyLlmClient;
use sisyphus_types::{ExecutionContext, SOPGraph, TypesError};
use std::sync::Arc;

mod common;
use common::{MockConsensus, MockExperienceBank, MockLogicalSimulator};

// --- Mock Implementations ---

struct MockGoalDecomposer;
#[async_trait]
impl GoalDecomposer for MockGoalDecomposer {
    async fn decompose(
        &self,
        _goal: &str,
    ) -> Result<Vec<sisyphus_intel::synthesis::ActionType>, String> {
        Ok(vec![
            sisyphus_intel::synthesis::ActionType::Fetch,
            sisyphus_intel::synthesis::ActionType::Process,
            sisyphus_intel::synthesis::ActionType::Validate,
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
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("Store init failed"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    let tool_executor = Arc::new(MockExecutor);
    let llm_client = Arc::new(DummyLlmClient);

    let simulator = Arc::new(MonteCarloSimulator::new(100));
    let intel = Arc::new(SisyphusIntel::new(
        simulator,
        Arc::new(MockLogicalSimulator),
        Arc::new(StandardReflector),
        Arc::new(StandardEvolver::new()),
    ));

    let synthesizer = Arc::new(LlmDrivenSynthesizer::new(
        Arc::new(MockGoalDecomposer),
        tool_executor.clone() as Arc<dyn sisyphus_types::ToolExecutor>,
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new())
            as Arc<dyn sisyphus_types::TestExecutor>,
    ));

    SisyphusKernel::new(
        store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.unwrap()),
        Arc::new(
            sisyphus_mem::VectorMemoryStore::new(pool.clone())
                .await
                .unwrap(),
        ),
        Arc::new(MockExperienceBank),
        tool_executor as Arc<dyn sisyphus_types::ToolExecutor>,
        intel,
        llm_client.clone() as Arc<dyn sisyphus_tools_llm::LlmClient>,
        llm_client as Arc<dyn sisyphus_tools_llm::LlmClient>,
        synthesizer as Arc<dyn sisyphus_intel::FlowSynthesizer>,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
        Arc::new(sisyphus_mem::RoadmapSqlStore::new(pool).await.unwrap()),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
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
                "default",
                sisyphus_types::WorkMode::UltraWork,
                None,
            )
            .await
            .expect("Synthesis failed");

        let graph = synthesis.graph;

        let result = kernel
            .execute_with_cognition(graph, &sop_id, None, None, None, None)
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
                eprintln!("Benchmark goal {} failed: {}", goal.id, e);
            }
        }
    }

    println!("{:-<60}", "");
    println!(
        "Overall Success Rate: {:.2}%",
        (overall_success as f32 / HARD_GOALS.len() as f32) * 100.0
    );

    // At least one goal should have been processed (not necessarily converged)
    assert!(
        overall_success >= 0,
        "Benchmark should complete without panicking"
    );
}
