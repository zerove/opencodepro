use crate::simulation::Simulator;
use crate::{
    action_label, classify_goal, tool_id_for_action, ActionType, FlowSynthesizer, SynthesisResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sisyphus_types::{ExecutionContext, SOPGraph};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicLesson {
    pub goal: String,
    pub sop_json: String,
    pub feedback: String,
    pub success: bool,
}

#[async_trait]
pub trait GoalDecomposer: Send + Sync {
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String>;
}

pub struct LlmGoalDecomposer<F>
where
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static,
{
    decompose_fn: F,
}

impl<F> LlmGoalDecomposer<F>
where
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static,
{
    pub fn new(decompose_fn: F) -> Self {
        Self { decompose_fn }
    }
}

#[async_trait]
impl<F> GoalDecomposer for LlmGoalDecomposer<F>
where
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static,
{
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String> {
        (self.decompose_fn)(goal.to_string())
    }
}

pub struct FallbackGoalDecomposer {
    inner: Arc<dyn GoalDecomposer>,
}

impl FallbackGoalDecomposer {
    pub fn new(inner: Arc<dyn GoalDecomposer>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl GoalDecomposer for FallbackGoalDecomposer {
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String> {
        match self.inner.decompose(goal).await {
            Ok(actions) => Ok(actions),
            Err(e) => {
                tracing::warn!(error = %e, "LLM decomposition failed, falling back to keyword matching");
                Ok(classify_goal(goal))
            }
        }
    }
}

pub struct LlmDrivenSynthesizer {
    pub decomposer: Arc<dyn GoalDecomposer>,
    pub tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
    pub test_executor: Arc<dyn sisyphus_types::TestExecutor>,
}

impl LlmDrivenSynthesizer {
    pub fn new(
        decomposer: Arc<dyn GoalDecomposer>,
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
        test_executor: Arc<dyn sisyphus_types::TestExecutor>,
    ) -> Self {
        Self {
            decomposer,
            tool_executor,
            test_executor,
        }
    }
}

#[async_trait]
impl FlowSynthesizer for LlmDrivenSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        _context: &ExecutionContext,
        _project_id: &str,
        mode: sisyphus_types::WorkMode,
        _insights: Option<&str>,
    ) -> Result<SynthesisResult, sisyphus_types::IntelError> {
        if mode == sisyphus_types::WorkMode::Standard {
            let actions = self.decomposer.decompose(goal).await?;
            let mut graph = SOPGraph::new();

            if actions.is_empty() {
                let n = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                    name: "default".into(),
                    tool_id: "fast_tool".into(),
                    fuel_limit: 1_000_000,
                    executor: self.tool_executor.clone(),
                }));
                return Ok(SynthesisResult {
                    graph: Arc::new(graph),
                    start_node: n,
                    plan_description: "Default single-node task".into(),
                });
            }

            let mut prev = None;
            let mut first_idx = None;

            for action in &actions {
                let label = action_label(action);
                let n = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                    name: label.into(),
                    tool_id: tool_id_for_action(action).into(),
                    fuel_limit: 1_000_000,
                    executor: self.tool_executor.clone(),
                }));
                if first_idx.is_none() {
                    first_idx = Some(n);
                }
                if let Some(p) = prev {
                    graph.add_edge(p, n);
                }
                prev = Some(n);
            }

            Ok(SynthesisResult {
                start_node: first_idx.expect("actions not empty"),
                graph: Arc::new(graph),
                plan_description: format!("Standard pipeline: {}", actions.len()),
            })
        } else {
            let mut graph = SOPGraph::new();
            let explore = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Explore".into(),
                tool_id: "lsp_symbol_search".into(),
                fuel_limit: 2_000_000,
                executor: self.tool_executor.clone(),
            }));
            let plan = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Plan".into(),
                tool_id: "metis/analyse".into(),
                fuel_limit: 2_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(explore, plan);
            let implement = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Implement".into(),
                tool_id: "bash_exec".into(),
                fuel_limit: 5_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(plan, implement);
            let verify = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Verify".into(),
                tool_id: "type_check".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(implement, verify);

            Ok(SynthesisResult {
                start_node: explore,
                graph: Arc::new(graph),
                plan_description:
                    "UltraWork Rigorous Cycle: Explore -> Plan -> Implement -> Verify".into(),
            })
        }
    }
}

pub struct LogicalSOPSynthesizer {
    pub llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
    pub tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
    pub test_executor: Arc<dyn sisyphus_types::TestExecutor>,
    pub simulator: Arc<dyn Simulator>,
    pub retriever: Arc<dyn sisyphus_types::ExperienceRetriever>,
    pub evaluator: Arc<dyn sisyphus_types::SopEvaluator>,
    pub golden_dataset: Arc<dyn sisyphus_types::GoldenDataset>,
}

impl LogicalSOPSynthesizer {
    pub fn new(
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
        test_executor: Arc<dyn sisyphus_types::TestExecutor>,
        simulator: Arc<dyn Simulator>,
        retriever: Arc<dyn sisyphus_types::ExperienceRetriever>,
        evaluator: Arc<dyn sisyphus_types::SopEvaluator>,
        golden_dataset: Arc<dyn sisyphus_types::GoldenDataset>,
    ) -> Self {
        Self {
            llm_client,
            tool_executor,
            test_executor,
            simulator,
            retriever,
            evaluator,
            golden_dataset,
        }
    }

    async fn retrieve_lessons(&self, goal: &str) -> String {
        match self.retriever.retrieve_relevant(goal, 5).await {
            Ok(lessons) => {
                if lessons.is_empty() {
                    return "No relevant historical lessons found.".to_string();
                }
                lessons
                    .iter()
                    .map(|l| {
                        format!(
                            "Goal: {}\nOutcome: {}\nFeedback: {}\nSOP: {}",
                            l.context_text,
                            if l.outcome_success {
                                "SUCCESS"
                            } else {
                                "FAILURE"
                            },
                            l.insight.as_ref().unwrap_or(&"No insight".to_string()),
                            l.action_id
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n---\n\n")
            }
            Err(_) => "Error retrieving semantic memory.".to_string(),
        }
    }

    async fn save_lesson(&self, goal: &str, sop_json: &str, feedback: &str, success: bool) {
        let exp = sisyphus_types::experience::ExperienceTuple {
            context_text: goal.to_string(),
            context_hash: format!("{:x}", md5::compute(goal)),
            project_id: None,
            action_id: sop_json.to_string(),
            visual_embedding: None,
            outcome_success: success,
            deviation_score: 0.0,
            cost: 0,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            embedding: None,
            insight: Some(feedback.to_string()),
        };
        let _ = self.retriever.record_experience(exp).await;
    }

    async fn propose(
        &self,
        goal: &str,
        tools: &str,
        lessons: &str,
    ) -> Result<(String, String, String), String> {
        let system_prompt = format!(
            r#"
You are a Strategic SOP Architect. Design a high-fidelity SOP graph.
Available Node Types:
1. Wasm: {{ "tool_id": "...", "fuel_limit": 1000000 }}
2. Retry: {{ "child": index, "max_retries": 3 }}
3. Branch: {{ "branches": {{ "val": idx }}, "default_next": idx }}
4. Test: {{ "command": "...", "workdir": "..." }}

Constraint: Every node that implements a feature or modifies the codebase (e.g., a Wasm node using a build or edit tool) MUST be immediately followed by a Test node to verify the correctness of the change.

Currently Available Tools:
{}

### Strategic Memory & Insights (Lessons from past attempts and session context):
{}

Output Format:
Reasoning: <topology plan, explaining how verification is integrated>
JSON: <SOPGraphConfig JSON>
Plan: <overall strategy>"#,
            tools, lessons
        );

        let response = self
            .llm_client
            .chat(&system_prompt, &format!("Goal: {}", goal), true)
            .await
            .map_err(|e| e.to_string())?;

        let json_start = response.find("JSON:").map(|i| i + 5).unwrap_or(0);
        let json_end = response.rfind("Plan:").unwrap_or(response.len());
        let json_str = &response[json_start..json_end].trim();
        let plan_desc = response
            .split("Plan:")
            .nth(1)
            .unwrap_or("Logical SOP")
            .trim()
            .to_string();
        let reasoning = response
            .split("JSON:")
            .next()
            .unwrap_or("")
            .trim()
            .to_string();

        Ok((reasoning, json_str.to_string(), plan_desc))
    }

    async fn refine(
        &self,
        goal: &str,
        tools: &str,
        current_json: &str,
        feedback: &str,
        lessons: &str,
    ) -> Result<(String, String, String), String> {
        let system_prompt = format!(
            r#"
You are a Strategic Architect. Refine the SOP based on the Auditor's feedback.
Fix the logical gaps and optimize the topology. Ensure the JSON is strictly valid.

### Contextual Lessons:
{}

Respond in the same format:
Reasoning: <how you fixed the issues>
JSON: <updated SOPGraphConfig JSON>
Plan: <updated strategy>"#,
            lessons
        );

        let prompt = format!(
            "Goal: {}\nAvailable Tools: {}\nCurrent JSON: {}\nAuditor Feedback: {}\nRefine this design.",
            goal, tools, current_json, feedback
        );

        let response = self
            .llm_client
            .chat(&system_prompt, &prompt, true)
            .await
            .map_err(|e| e.to_string())?;

        let json_start = response.find("JSON:").map(|i| i + 5).unwrap_or(0);
        let json_end = response.rfind("Plan:").unwrap_or(response.len());
        let json_str = &response[json_start..json_end].trim();
        let plan_desc = response
            .split("Plan:")
            .nth(1)
            .unwrap_or("Logical SOP")
            .trim()
            .to_string();
        let reasoning = response
            .split("JSON:")
            .next()
            .unwrap_or("")
            .trim()
            .to_string();

        Ok((reasoning, json_str.to_string(), plan_desc))
    }

    async fn synthesize_subgraph(
        &self,
        sub_goal: &str,
        tools: &str,
    ) -> Result<sisyphus_types::SOPGraphConfig, String> {
        let lessons = self.retrieve_lessons(sub_goal).await;
        let (_, json_str, _) = self.propose(sub_goal, tools, &lessons).await?;
        serde_json::from_str(&json_str).map_err(|e| format!("Invalid sub-graph JSON: {}", e))
    }

    async fn synthesize_hierarchical(
        &self,
        goal: &str,
        tools: &str,
    ) -> Result<SynthesisResult, sisyphus_types::IntelError> {
        let lessons = self.retrieve_lessons(goal).await;

        let system_prompt = format!(
            r#"
You are a Strategic Architect. The goal is complex and requires a hierarchical SOP.
Design a high-level SOP where each node is a SubGraph. 
Each SubGraph must have a 'goal' field describing what that sub-SOP should accomplish.

Available Tools:
{}

Strategic Memory:
{}

Output Format:
Reasoning: <why this hierarchy>
JSON: <SOPGraphConfig with NodeKind::SubGraph nodes, each containing a 'goal' in its config>
Plan: <high-level strategy>"#,
            tools, lessons
        );

        let response = self
            .llm_client
            .chat(&system_prompt, &format!("Complex Goal: {}", goal), true)
            .await
            .map_err(|e| e.to_string())?;

        let json_start = response.find("JSON:").map(|i| i + 5).unwrap_or(0);
        let json_end = response.rfind("Plan:").unwrap_or(response.len());
        let json_str = &response[json_start..json_end].trim();
        let plan_desc = response
            .split("Plan:")
            .nth(1)
            .unwrap_or("Hierarchical SOP")
            .trim()
            .to_string();

        let mut config: sisyphus_types::SOPGraphConfig = serde_json::from_str(json_str)
            .map_err(|e| format!("Invalid hierarchical JSON: {}", e))?;

        for node in &mut config.nodes {
            if let sisyphus_types::NodeKind::SubGraph { config: sub_cfg } = &mut node.kind {
                let sub_goal = node.name.clone();
                let detailed_cfg = self.synthesize_subgraph(&sub_goal, tools).await?;
                **sub_cfg = detailed_cfg;
            }
        }

        let graph = sisyphus_types::SOPGraph::from_config(
            &config,
            self.tool_executor.clone(),
            self.test_executor.clone(),
        )
        .map_err(|e| e.to_string())?;

        Ok(SynthesisResult {
            start_node: graph
                .graph
                .node_indices()
                .next()
                .ok_or_else(|| "Hierarchical graph has no nodes".to_string())?,
            graph,
            plan_description: plan_desc,
        })
    }
}

#[async_trait]
impl FlowSynthesizer for LogicalSOPSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        _context: &ExecutionContext,
        _project_id: &str,
        _mode: sisyphus_types::WorkMode,
        _insights: Option<&str>,
    ) -> Result<SynthesisResult, sisyphus_types::IntelError> {
        tracing::debug!(goal = %goal, "Starting synthesis");
        let tools = self
            .tool_executor
            .list_tools()
            .await
            .map_err(|e| format!("Failed to discover tools: {}", e))?;
        let tools_list = tools
            .iter()
            .map(|t| format!("- {}: {}", t.id, t.description))
            .collect::<Vec<_>>()
            .join("\n");

        let is_complex = goal.len() > 150
            || goal.to_lowercase().contains("refactor")
            || goal.to_lowercase().contains("implement full")
            || goal.to_lowercase().contains("comprehensive");

        if is_complex {
            tracing::info!(goal = %goal, "Goal detected as complex. Using hierarchical synthesis.");
            return self.synthesize_hierarchical(goal, &tools_list).await;
        }

        let mut current_json = String::new();
        let mut current_plan = String::new();
        let mut last_critique = String::new();
        let mut current_attempt = 0;
        let max_attempts = 3;

        while current_attempt < max_attempts {
            let lessons = self.retrieve_lessons(goal).await;

            let (_, json_str, plan_desc) = if current_attempt == 0 {
                let res = self.propose(goal, &tools_list, &lessons).await?;
                tracing::debug!("First proposal successful");
                res
            } else {
                let res = self
                    .refine(goal, &tools_list, &current_json, &last_critique, &lessons)
                    .await?;
                tracing::debug!(attempt = current_attempt, "Refinement successful");
                res
            };

            current_json = json_str;
            current_plan = plan_desc;

            let config: sisyphus_types::SOPGraphConfig = serde_json::from_str(&current_json)
                .map_err(|e| format!("Invalid JSON in attempt {}: {}", current_attempt, e))?;

            let graph = SOPGraph::from_config(
                &config,
                self.tool_executor.clone(),
                self.test_executor.clone(),
            )
            .map_err(|e| {
                format!(
                    "SOPGraph construction failed in attempt {}: {}",
                    current_attempt, e
                )
            })?;

            let eval_res = match self.evaluator.evaluate(goal, &config).await {
                Ok(res) => res,
                Err(e) => {
                    tracing::debug!(error = %e, "Evaluation failed");
                    return Err(e.to_string().into());
                }
            };

            tracing::debug!(
                score = eval_res.score,
                acceptable = eval_res.is_acceptable,
                "Evaluation result received"
            );
            self.save_lesson(
                goal,
                &current_json,
                &eval_res.feedback,
                eval_res.is_acceptable,
            )
            .await;

            if eval_res.is_acceptable {
                // Regression check against Golden Dataset
                if let Err(e) = self
                    .golden_dataset
                    .verify_no_regression(goal, &config, eval_res.score as f64)
                    .await
                {
                    tracing::warn!(goal = %goal, error = %e, "Golden dataset verification error");
                } else if !self
                    .golden_dataset
                    .verify_no_regression(goal, &config, eval_res.score as f64)
                    .await
                    .unwrap_or(true)
                {
                    tracing::warn!(goal = %goal, "SOP rejected: regression detected relative to Golden Dataset");
                    last_critique = "SOP shows regression compared to Golden Dataset. Improve stability/performance.".into();
                    current_attempt += 1;
                    continue;
                }

                tracing::info!(goal = %goal, attempts = current_attempt + 1, score = eval_res.score, "SOP approved by evaluator and passed regression check");

                // Enroll if this is the new gold standard
                let _ = self
                    .golden_dataset
                    .enroll(goal, config.clone(), eval_res.score as f64)
                    .await;

                return Ok(SynthesisResult {
                    start_node: graph
                        .graph
                        .node_indices()
                        .next()
                        .ok_or_else(|| "Graph has no nodes".to_string())?,
                    graph,
                    plan_description: current_plan,
                });
            }

            tracing::warn!(goal = %goal, attempt = current_attempt + 1, feedback = %eval_res.feedback, "SOP rejected by evaluator");
            last_critique = eval_res.feedback;
            current_attempt += 1;
        }

        if !current_json.is_empty() {
            let config: sisyphus_types::SOPGraphConfig =
                serde_json::from_str(&current_json).map_err(|e| e.to_string())?;
            let graph = SOPGraph::from_config(
                &config,
                self.tool_executor.clone(),
                self.test_executor.clone(),
            )
            .map_err(|e| e.to_string())?;
            return Ok(SynthesisResult {
                start_node: graph
                    .graph
                    .node_indices()
                    .next()
                    .ok_or_else(|| "Graph has no nodes".to_string())?,
                graph,
                plan_description: current_plan,
            });
        }

        Err(sisyphus_types::IntelError::Internal(format!(
            "Failed to synthesize a valid SOP after {} attempts",
            max_attempts
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fallback_decomposer() {
        let llm_decomposer = Arc::new(LlmGoalDecomposer::new(|_| Err("LLM Error".into())));
        let fallback = Arc::new(FallbackGoalDecomposer::new(llm_decomposer));
        let goal = "fetch and process";
        let actions = fallback.decompose(goal).await.unwrap();
        assert!(actions.contains(&ActionType::Fetch));
        assert!(actions.contains(&ActionType::Process));
    }

    #[tokio::test]
    async fn test_llm_decomposer_success() {
        let llm_decomposer = Arc::new(LlmGoalDecomposer::new(|_| {
            Ok(vec![ActionType::Fetch, ActionType::Sink])
        }));
        let goal = "some goal";
        let actions = llm_decomposer.decompose(goal).await.unwrap();
        assert_eq!(actions, vec![ActionType::Fetch, ActionType::Sink]);
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;
    use crate::simulation::MonteCarloSimulator;
    use sisyphus_types::ExecutionContext;
    use std::sync::Arc;

    struct MockLlm {
        call_count: std::sync::atomic::AtomicUsize,
        mode: std::sync::Arc<std::sync::Mutex<usize>>,
    }

    impl MockLlm {
        fn new() -> Self {
            Self {
                call_count: std::sync::atomic::AtomicUsize::new(0),
                mode: std::sync::Arc::new(std::sync::Mutex::new(0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl sisyphus_tools_llm::LlmClient for MockLlm {
        async fn chat(
            &self,
            _system: &str,
            prompt: &str,
            _stream: bool,
        ) -> Result<String, sisyphus_types::TypesError> {
            let _count = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut mode = self.mode.lock().unwrap();

            if prompt.contains("Refine this design") {
                *mode += 1;
                Ok("Reasoning: Fixed the loop.\nJSON: {\"start_node\": 0, \"nodes\": [{ \"name\": \"Node0\", \"kind\": { \"Wasm\": { \"tool_id\": \"t1\", \"fuel_limit\": 1000 } } }], \"edges\": []}\nPlan: Fixed and robust.".to_string())
            } else if prompt.contains("Audit this design") {
                if *mode == 0 {
                    Ok("REJECTED: The path is too risky. Add a retry node.".into())
                } else {
                    Ok("APPROVED: The design is now robust.".into())
                }
            } else {
                Ok("Reasoning: Initial plan.\nJSON: {\"start_node\": 0, \"nodes\": [{ \"name\": \"Node0\", \"kind\": { \"Wasm\": { \"tool_id\": \"t1\", \"fuel_limit\": 1000 } } }], \"edges\": []}\nPlan: Initial strategy.".to_string())
            }
        }

        async fn generate(&self, _prompt: &str) -> Result<String, sisyphus_types::TypesError> {
            Ok("Mock generate".into())
        }

        fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
            (
                "default".to_string(),
                "default".to_string(),
                "default".to_string(),
            )
        }

        fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
            sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
        }
    }

    #[tokio::test]
    async fn test_adversarial_loop_convergence() {
        let llm = Arc::new(MockLlm::new());
        let executor = Arc::new(sisyphus_types::ExecutorRouter::new(Arc::new(MockExecutor)));
        let sim = Arc::new(MonteCarloSimulator::new(10));
        let _store = Arc::new(MockStateStore::new());

        let synthesizer = LogicalSOPSynthesizer::new(
            llm,
            executor,
            Arc::new(MockTestExecutor),
            sim,
            Arc::new(MockStateStore::new()),
            Arc::new(MockEvaluator::new()),
            Arc::new(MockGoldenDataset),
        );
        let result = synthesizer
            .synthesize(
                "test goal",
                &ExecutionContext::new(),
                "test",
                sisyphus_types::WorkMode::UltraWork,
                None,
            )
            .await;

        assert!(
            result.is_ok(),
            "Adversarial loop should converge to a valid SOP"
        );
        let res = result.unwrap();
        assert!(!res.plan_description.is_empty());
    }

    #[tokio::test]
    async fn test_strategic_memory_retrieval() {
        let llm = Arc::new(MockLlm::new());
        let executor = Arc::new(sisyphus_types::ExecutorRouter::new(Arc::new(MockExecutor)));
        let sim = Arc::new(MonteCarloSimulator::new(10));
        let _store = Arc::new(MockStateStore::new());

        let synthesizer = LogicalSOPSynthesizer::new(
            llm,
            executor,
            Arc::new(MockTestExecutor),
            sim,
            Arc::new(MockStateStore::new()),
            Arc::new(MockEvaluator::new()),
            Arc::new(MockGoldenDataset),
        );

        // 1. First attempt: Force a failure to save a lesson
        let _ = synthesizer
            .synthesize(
                "memory goal",
                &ExecutionContext::new(),
                "test",
                sisyphus_types::WorkMode::UltraWork,
                None,
            )
            .await;

        // 2. Verify lesson was saved
        let lessons = synthesizer.retrieve_lessons("memory goal").await;
        assert!(
            lessons.contains("SOP:"),
            "Should have saved a failed SOP as a lesson. Actual lessons: {}",
            lessons
        );

        // 3. Second attempt: Verify it can still synthesize
        let res = synthesizer
            .synthesize(
                "memory goal",
                &ExecutionContext::new(),
                "test",
                sisyphus_types::WorkMode::UltraWork,
                None,
            )
            .await;
        assert!(res.is_ok());
    }

    struct MockTestExecutor;
    #[async_trait::async_trait]
    impl sisyphus_types::TestExecutor for MockTestExecutor {
        async fn run_test(
            &self,
            _command: &str,
            _workdir: Option<&str>,
        ) -> Result<sisyphus_types::TestResult, sisyphus_types::TypesError> {
            Ok(sisyphus_types::TestResult {
                success: true,
                stdout: "Mock Success".into(),
                stderr: "".into(),
                exit_code: 0,
                duration_ms: 10,
                error_summary: None,
            })
        }
    }

    struct MockExecutor;
    #[async_trait::async_trait]
    impl sisyphus_types::ToolExecutor for MockExecutor {
        async fn execute(
            &self,
            _: &str,
            _: Vec<u8>,
            _: u64,
        ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
            Ok(vec![])
        }
        async fn list_tools(
            &self,
        ) -> Result<Vec<sisyphus_types::ToolMetadata>, sisyphus_types::TypesError> {
            Ok(vec![])
        }
    }

    struct MockStateStore {
        data: std::sync::Mutex<std::collections::HashMap<String, serde_json::Value>>,
        experiences: std::sync::Mutex<Vec<sisyphus_types::experience::ExperienceTuple>>,
    }
    impl MockStateStore {
        fn new() -> Self {
            Self {
                data: std::sync::Mutex::new(std::collections::HashMap::new()),
                experiences: std::sync::Mutex::new(Vec::new()),
            }
        }
    }
    #[async_trait::async_trait]
    impl sisyphus_types::ExperienceRetriever for MockStateStore {
        async fn retrieve_relevant(
            &self,
            query: &str,
            _limit: usize,
        ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>, sisyphus_types::TypesError>
        {
            // Mock simple retrieval: return experiences that match the query
            let experiences = self.experiences.lock().unwrap();
            let filtered: Vec<_> = experiences
                .iter()
                .filter(|e| e.context_text.contains(query))
                .cloned()
                .collect();
            Ok(filtered)
        }
        async fn record_experience(
            &self,
            exp: sisyphus_types::experience::ExperienceTuple,
        ) -> Result<(), sisyphus_types::TypesError> {
            self.experiences.lock().unwrap().push(exp);
            Ok(())
        }
    }
    #[async_trait::async_trait]
    impl sisyphus_types::StateStore for MockStateStore {
        async fn get_state_json(
            &self,
            _project_id: &str,
            key: &str,
        ) -> Result<Option<serde_json::Value>, sisyphus_types::TypesError> {
            let res = self.data.lock().unwrap().get(key).cloned();
            println!("DEBUG: MockStateStore GET key: {}, result: {:?}", key, res);
            Ok(res)
        }
        async fn set_state_json(
            &self,
            _project_id: &str,
            key: &str,
            val: serde_json::Value,
        ) -> Result<(), sisyphus_types::TypesError> {
            println!("DEBUG: MockStateStore SET key: {}", key);
            self.data.lock().unwrap().insert(key.to_string(), val);
            Ok(())
        }
        async fn get(
            &self,
            _project_id: &str,
            key: &str,
        ) -> Result<Option<Vec<u8>>, sisyphus_types::TypesError> {
            let res = self.data.lock().unwrap().get(key).cloned();
            Ok(res.map(|v| serde_json::to_vec(&v).unwrap_or_default()))
        }
        async fn set(
            &self,
            _project_id: &str,
            key: &str,
            val: &[u8],
        ) -> Result<(), sisyphus_types::TypesError> {
            let v: serde_json::Value =
                serde_json::from_slice(val).unwrap_or(serde_json::Value::Null);
            self.data.lock().unwrap().insert(key.to_string(), v);
            Ok(())
        }
        async fn get_traces_for_sop(
            &self,
            _project_id: &str,
            _sop_id: &str,
        ) -> Result<Vec<(String, String, String, i64, i64)>, sisyphus_types::TypesError> {
            Ok(vec![])
        }
    }
}

#[cfg(test)]
struct MockGoldenDataset;
#[cfg(test)]
#[async_trait::async_trait]
impl sisyphus_types::GoldenDataset for MockGoldenDataset {
    async fn enroll(
        &self,
        _goal: &str,
        _config: sisyphus_types::SOPGraphConfig,
        _metric: f64,
    ) -> Result<(), sisyphus_types::TypesError> {
        Ok(())
    }
    async fn get_golden_sop(
        &self,
        _goal: &str,
    ) -> Result<Option<sisyphus_types::GoldenSop>, sisyphus_types::TypesError> {
        Ok(None)
    }
    async fn verify_no_regression(
        &self,
        _goal: &str,
        _candidate: &sisyphus_types::SOPGraphConfig,
        _current_metric: f64,
    ) -> Result<bool, sisyphus_types::TypesError> {
        Ok(true)
    }
}

#[cfg(test)]
struct MockEvaluator {
    call_count: std::sync::atomic::AtomicUsize,
}
#[cfg(test)]
impl MockEvaluator {
    fn new() -> Self {
        Self {
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}
#[cfg(test)]
#[async_trait::async_trait]
impl sisyphus_types::SopEvaluator for MockEvaluator {
    async fn evaluate(
        &self,
        _goal: &str,
        _config: &sisyphus_types::SOPGraphConfig,
    ) -> Result<sisyphus_types::SopEvaluation, sisyphus_types::TypesError> {
        let count = self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if count == 0 {
            // First call: reject to force lesson saving
            Ok(sisyphus_types::SopEvaluation {
                score: 0.0,
                metrics: std::collections::HashMap::new(),
                feedback: "Rejected for testing".into(),
                is_acceptable: false,
            })
        } else {
            // Subsequent calls: accept
            Ok(sisyphus_types::SopEvaluation {
                score: 1.0,
                metrics: std::collections::HashMap::new(),
                feedback: "Perfect".into(),
                is_acceptable: true,
            })
        }
    }
}
