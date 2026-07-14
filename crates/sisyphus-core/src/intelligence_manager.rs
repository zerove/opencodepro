use std::sync::Arc;
use sisyphus_intel::{SisyphusIntel, SimulationResult, ReflectionReport};
use sisyphus_types::{SOPGraph, ExecutionContext, NodeOutput};
use async_trait::async_trait;
use sisyphus_tools_llm_tools::{oracle, momus};
use std::path::Path;


/// IntelligenceManager wraps the cognitive capabilities of the system,
/// providing high-level APIs for prediction, reflection, and evolution.
pub struct IntelligenceManager {
    pub intel: Arc<SisyphusIntel>,
    pub synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
    pub llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
}

impl IntelligenceManager {
    pub fn new(
        intel: Arc<SisyphusIntel>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
    ) -> Self {
        Self {
            intel,
            synthesizer,
            llm_client,
        }
    }

    /// Synthesize a new SOP graph from a natural language goal.
    pub async fn synthesize_goal(
        &self,
        goal: &str,
        ctx: &ExecutionContext,
        project_id: &str,
        mode: sisyphus_types::WorkMode,
        insights: Option<&str>,
    ) -> Result<sisyphus_intel::synthesis::SynthesisResult, String> {
        self.synthesizer
                    .synthesize(goal, ctx, project_id, mode, insights)
            .await
            .map_err(|e| e.to_string())
    }

    /// Predict the outcome of a graph execution using the simulator.
    pub async fn predict_outcome(
        &self,
        graph: Arc<SOPGraph>,
        ctx: &ExecutionContext,
        project_id: &str,
        sop_id: &str,
    ) -> Result<SimulationResult, String> {
        self.intel
            .simulator
            .simulate(graph, ctx, project_id, sop_id, None)
            .await
    }

    /// Reflect on an execution trace to calculate deviation and suggest improvements.
    pub async fn reflect_on_trace(
        &self,
        trace: &str,
        prediction: &SimulationResult,
        ctx: &ExecutionContext,
    ) -> Result<ReflectionReport, String> {
        // 1. Base reflection from the core reflector
        let mut report = self.intel
            .reflector
            .reflect(trace, prediction, None)
            .await?;

        // 2. Deep Semantic Reflection using Professional Tools
        let mut expert_suggestions = Vec::new();

        // a. Oracle Analysis: Check all modified files for complexity and security issues
        if let Ok(files) = ctx.modified_files.read() {
            for path in files.iter() {
                if let Ok(analysis) = oracle::analyze_file(path) {
                    for issue in analysis.issues {
                        expert_suggestions.push(format!(
                            "[{}] {} at {}:{}: {}", 
                            issue.severity, issue.message, 
                            path.display(), issue.line, 
                            issue.code.as_deref().unwrap_or("")
                        ));
                    }
                }
            }
        }

        // b. Momus Review: Review plan-like or code-like outputs in results
        for entry in ctx.results.iter() {
            let node_name = entry.key();
            let output = entry.value();
            let content = output.to_string();
            
            // Heuristic: if node name contains 'plan' or output is long and structured, use Momus Plan Review
            if node_name.contains("plan") || content.contains("## ") {
                let review = momus::review_plan(self.llm_client.as_ref(), &content, node_name.as_str()).await;
                for comment in review.comments {
                        expert_suggestions.push(format!(
                            "[{}] (Plan Review) {}: {} - Suggestion: {}", 
                            comment.severity, comment.category, comment.message, 
                            comment.suggestion.as_deref().unwrap_or("N/A")
                        ));
                    }
            }
            
            // If output looks like code (contains common Rust keywords), use Momus Code Review
            if content.contains("fn ") || content.contains("pub struct") {
                // We need a path for review_code. We'll use a virtual path since it's in-memory.
                let path_str = format!("/virtual/{}", node_name);
                let virtual_path = Path::new(&path_str);
                if let Ok(review) = momus::review_code(self.llm_client.as_ref(), virtual_path, &content).await {
                    for comment in review.comments {
                        expert_suggestions.push(format!(
                            "[{}] (Code Review) {}: {} - Suggestion: {}", 
                            comment.severity, comment.category, comment.message, 
                            comment.suggestion.as_deref().unwrap_or("N/A")
                        ));
                    }
                }
            }
        }

        // Merge expert findings into the report
        report.suggested_improvements.extend(expert_suggestions);

        Ok(report)
    }

    /// Critically analyze a reflection report to identify risks or blind spots.
    pub async fn critique_reflection(
        &self,
        report: &ReflectionReport,
        prediction: &SimulationResult,
    ) -> Result<sisyphus_types::CritiqueResult, String> {
        let prompt = format!(
            "You are a rigorous AI Architecture Critic. Analyze the following reflection report and simulation prediction.\n\n\
            Simulation Prediction:\n{:?}\n\n\
            Reflection Report:\n{:?}\n\n\
            Task: Identify any logical fallacies, over-optimistic assumptions, or missing edge cases in the suggested improvements. \
            Determine if the proposed evolution is safe or if it might introduce new regressions.\n\n\
            Return your response in JSON format with the following fields:\n\
            - critical_findings: List of specific concerns\n\
            - risk_score: 0.0 to 1.0\n\
            - suggested_refinement: How to improve the evolution plan\n\
            - should_proceed: boolean",
            prediction, report
        );

        let response = self.llm_client.generate(&prompt).await
            .map_err(|e| format!("LLM critique failed: {}", e))?;
        
        serde_json::from_str::<sisyphus_types::CritiqueResult>(&response)
            .map_err(|e| format!("Failed to parse critique JSON: {}. Response was: {}", e, response))
    }
    /// Evolve a graph based on a reflection report and prediction.
    pub async fn evolve_graph(
        &self,
        graph: Arc<SOPGraph>,
        report: &ReflectionReport,
        prediction: &SimulationResult,
        insights: Option<&str>,
    ) -> Result<Arc<SOPGraph>, String> {
        self.intel
            .evolver
            .evolve(graph, report, prediction, insights)
            .await
    }
}
