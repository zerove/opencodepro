//! Shared test helpers for sisyphus-core integration tests.
//!
//! Provides mock implementations and a builder for constructing a test
//! `SisyphusKernel` without duplicating boilerplate across test files.

// 本模块被 12 个集成测试各自 `mod common;` 引入，每个测试只用到其中一部分
// 夹具，未使用的成员按文件维度被判为死代码。夹具集成本就是共享的，
// 因此在模块层级豁免，而不是给每个成员单独标注。
#![allow(dead_code)]

use async_trait::async_trait;
use sisyphus_core::{Consensus, SOPGraph, WasmSOPNode};
use sisyphus_intel::simulation::LogicalSimulationResult;
use sisyphus_intel::simulation::LogicalSimulator;
use sisyphus_intel::{ReflectionReport, SimulationResult};
use sisyphus_types::{experience::ExperienceTuple, ExecutionContext, ToolExecutor, WorkMode};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Mock Consensus
// ---------------------------------------------------------------------------

/// Mock Consensus.
pub struct MockConsensus;

#[async_trait]
impl Consensus for MockConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "mock-leader".into()
    }
    async fn propose(
        &self,
        _key: &str,
        _value: serde_json::Value,
    ) -> Result<(), sisyphus_types::TypesError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock LogicalSimulator
// ---------------------------------------------------------------------------

/// Mock Logical Simulator.
pub struct MockLogicalSimulator;

#[async_trait]
impl LogicalSimulator for MockLogicalSimulator {
    async fn simulate_logic(
        &self,
        _graph: Arc<SOPGraph>,
        _context: &ExecutionContext,
        _sop_id: &str,
        _goal_text: Option<&str>,
    ) -> Result<LogicalSimulationResult, sisyphus_types::IntelError> {
        Ok(LogicalSimulationResult {
            is_valid: true,
            flaws: vec![],
            predicted_path: vec!["mock".to_string()],
            confidence: 1.0,
        })
    }
}

// ---------------------------------------------------------------------------
// Mock Reflector
// ---------------------------------------------------------------------------

/// Mock Reflector.
pub struct MockReflector;

#[async_trait]
impl sisyphus_intel::reflect::Reflector for MockReflector {
    async fn reflect(
        &self,
        _trace: &str,
        _predicted: &SimulationResult,
        _experiences: Option<&[ExperienceTuple]>,
    ) -> Result<ReflectionReport, sisyphus_types::IntelError> {
        Ok(ReflectionReport {
            original_sop_id: "mock".into(),
            deviation_score: 0.0,
            suggested_improvements: vec![],
            improvement_signals: vec![],
            evolved_sop_structure: None,
            records: vec![],
        })
    }
}

// ---------------------------------------------------------------------------
// Mock SOPEvolver
// ---------------------------------------------------------------------------

/// Mock Evolver.
pub struct MockEvolver;

#[async_trait]
impl sisyphus_intel::reflect::SOPEvolver for MockEvolver {
    async fn evolve(
        &self,
        graph: Arc<SOPGraph>,
        _report: &ReflectionReport,
        _prediction: &SimulationResult,
        _insights: Option<&str>,
    ) -> Result<Arc<SOPGraph>, sisyphus_types::IntelError> {
        Ok(graph)
    }
}

// ---------------------------------------------------------------------------
// Mock ExperienceBank
// ---------------------------------------------------------------------------

/// Mock Experience Bank.
pub struct MockExperienceBank;

#[async_trait]
impl sisyphus_types::experience::ExperienceBank for MockExperienceBank {
    async fn record(&self, _exp: ExperienceTuple) -> Result<(), sisyphus_types::TypesError> {
        Ok(())
    }
    async fn query_similar(
        &self,
        _query: sisyphus_types::experience::ExperienceQuery,
    ) -> Result<Vec<ExperienceTuple>, sisyphus_types::TypesError> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// Mock FlowSynthesizer — returns a single-node graph
// ---------------------------------------------------------------------------

/// Mock Synthesizer.
pub struct MockSynthesizer;

#[async_trait]
impl sisyphus_intel::FlowSynthesizer for MockSynthesizer {
    async fn synthesize(
        &self,
        _goal: &str,
        _context: &ExecutionContext,
        _project_id: &str,
        _mode: WorkMode,
        _insights: Option<&str>,
    ) -> Result<sisyphus_intel::SynthesisResult, sisyphus_types::IntelError> {
        let mut graph = SOPGraph::new();
        let start_node = graph.add_node(Arc::new(WasmSOPNode {
            name: "mock_node".into(),
            tool_id: "mock_tool".into(),
            fuel_limit: 1_000_000,
            executor: Arc::new(MockExecutor::new()),
            args: Vec::new(),
        }));
        Ok(sisyphus_intel::SynthesisResult {
            graph: Arc::new(graph),
            start_node,
            plan_description: "mock plan".into(),
        })
    }
}

/// Mock Executor.
pub struct MockExecutor {
    pub call_count: AtomicUsize,
}

impl MockExecutor {
    /// Create a fresh in-memory test fixture for sisyphus-core integration tests.
    pub fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(format!("executed:{}", tool_id).into_bytes())
    }
}
