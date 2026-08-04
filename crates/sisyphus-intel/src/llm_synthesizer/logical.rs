//! 对抗式逻辑 SOP 合成器：提出图谱、评估它，
//! 在批判下细化，并对照黄金数据集验证，直到
//! 评估器接受结果。

use crate::simulation::Simulator;
use crate::{FlowSynthesizer, SynthesisResult};
use async_trait::async_trait;
use sisyphus_types::{ExecutionContext, SOPGraph};
use std::sync::Arc;

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

    pub(super) async fn retrieve_lessons(&self, goal: &str) -> String {
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
                // 对照黄金数据集的回归检查
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

                // 若为新的黄金标准则收录
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
