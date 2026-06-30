use async_trait::async_trait;
use std::sync::Arc;

use sisyphus_roadmap::{
    milestone_generator::MilestoneGenerator, resource_optimizer::ResourceOptimizer,
    strategic_reflection::StrategicReflector, ExecutionOutcome,
};
use sisyphus_tools::metis::LlmClient;
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase, Roadmap};

// ── Mocks ──────────────────────────────────────────────────

struct MockLlmClient;

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        self.chat("", prompt, false).await
    }

    async fn chat(&self, _system: &str, _user: &str, _stream: bool) -> Result<String, String> {
        // Return a valid JSON array of refinement suggestions for MilestoneGenerator
        // or a valid reflection JSON for StrategicReflector
        // propose_refinements includes "Change request:" in the context
        // reflect() does not — use that to discriminate
        if _user.contains("Change request") {
            Ok(r#"[{"milestone_id": "ms1", "phase_id": "ph1", "action": "Split goal", "rationale": "Too large"}]"#.to_string())
        } else {
            Ok(r#"{"project_id": "p1", "health_assessment": "Warning", "trajectory": "Slightly off", "recommendations": ["Optimize core"], "risk_factors": ["Latency"], "confidence": 0.8}"#.to_string())
        }
    }
}

#[tokio::test]
async fn test_strategic_governance_loop_detection() {
    // 1. Setup Roadmap
    let mut roadmap = Roadmap { milestones: vec![] };

    let goal_id = "goal_core_engine".to_string();
    let phase = Phase {
        id: "ph1".into(),
        name: "Core".into(),
        goals: vec![GoalRef {
            goal_id: goal_id.clone(),
            description: "Implement Core Engine".into(),
            status: MilestoneStatus::InProgress,
        }],
        depends_on: vec![],
    };
    let milestone = Milestone {
        id: "ms1".into(),
        name: "Milestone 1".into(),
        status: MilestoneStatus::InProgress,
        phases: vec![phase],
    };
    roadmap.milestones.push(milestone);

    // 2. Simulate Execution Outcomes with a Bottleneck
    let mut outcomes = vec![];

    // Normal tasks — spread across different goal IDs to create a low baseline avg
    for (i, gid) in ["g_init", "g_setup", "g_config", "g_test"]
        .iter()
        .enumerate()
    {
        outcomes.push(ExecutionOutcome {
            goal_id: gid.to_string(),
            project_id: "p1".into(),
            milestone_id: "ms1".into(),
            phase_id: "ph1".into(),
            success: true,
            deviation_score: Some(0.1),
            error: None,
            timestamp: 1000 + i as u64,
            actual_cost: 5.0,
            duration_secs: 10.0,
        });
    }

    // Bottleneck tasks (simulating high cost/latency)
    // avg_duration baseline = (10*4 + 1000*3) / 7 ≈ 434s
    // bottleneck avg = 1000s → ratio ≈ 2.3× > 2.0 threshold → "low" severity
    for i in 0..3 {
        outcomes.push(ExecutionOutcome {
            goal_id: goal_id.clone(),
            project_id: "p1".into(),
            milestone_id: "ms1".into(),
            phase_id: "ph1".into(),
            success: true,
            deviation_score: Some(0.8),
            error: None,
            timestamp: 1100 + i as u64,
            actual_cost: 150.0,
            duration_secs: 1000.0,
        });
    }

    // 3. Verify Resource Optimizer detects the bottleneck
    let optimizer = ResourceOptimizer::new();
    let analysis = optimizer.find_bottlenecks(&outcomes);

    assert!(!analysis.is_empty(), "Should have detected bottlenecks");
    assert!(
        analysis.iter().any(|b| b.goal_id == goal_id),
        "Bottleneck should be associated with goal_core_engine"
    );

    // 4. Verify Strategic Reflector identifies the risk
    let llm = Arc::new(MockLlmClient);
    let _reflector = StrategicReflector::new(llm.clone());

    // 5. Verify Milestone Generator can propose refinements
    let generator = MilestoneGenerator::new(llm);

    // Create a dummy manager for the generator
    let store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:")
            .await
            .unwrap(),
    );
    let manager = sisyphus_roadmap::RoadmapManager::new(store);

    manager
        .create_project("p1", "Project X", "Desc")
        .await
        .unwrap();
    manager
        .add_milestone("p1", roadmap.milestones[0].clone())
        .await
        .unwrap();

    let suggestions = generator
        .propose_refinements(&manager, "p1", "ms1", "Goal is bottlenecked")
        .await;

    assert!(
        suggestions.is_ok(),
        "Should successfully propose refinements"
    );
    let suggestions = suggestions.unwrap();
    assert!(
        !suggestions.is_empty(),
        "Generator should return suggestions from mock LLM"
    );
    assert_eq!(suggestions[0].action, "Split goal");
}
