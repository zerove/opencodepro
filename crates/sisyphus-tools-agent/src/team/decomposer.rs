//! LLM-based goal decomposition and result synthesis for team plans.
//!
//! This module provides the LLM-powered capabilities:
//! - [`decompose_goal`] breaks a high-level [`TeamGoal`] into a DAG of [`TaskNode`]s.
//! - [`synthesize`] consolidates multiple [`TaskResult`]s into a coherent summary.

use super::error::TeamError;
use super::orchestrator::TeamOrchestrator;
use super::parse::parse_decomposition_response;
use super::{TaskResult, TeamGoal, TeamPlan};
use sisyphus_tools_llm::metis::LlmClient;

impl TeamOrchestrator {
    /// Use an LLM to synthesize task results into a coherent summary.
    pub(super) async fn synthesize(
        &self,
        llm: &dyn LlmClient,
        plan: &TeamPlan,
        results: &[TaskResult],
    ) -> Option<String> {
        let mut summary_parts: Vec<String> = Vec::new();
        for result in results {
            let status = if result.success { "✓" } else { "✗" };
            let output_str = result
                .output
                .as_ref()
                .map(std::string::ToString::to_string)
                .unwrap_or_default();
            let snippet = if output_str.len() > 200 {
                format!("{}...", &output_str[..200])
            } else {
                output_str
            };
            summary_parts.push(format!(
                "{} [{}] {}: {}",
                status, result.agent_id, result.task_id, snippet
            ));
        }

        let prompt = format!(
            "You are a team lead synthesizing results from multiple specialist agents.\n\n\
             Goal: {}\n\n\
             Task Results:\n{}\n\n\
             Provide a concise synthesis of what was accomplished, key findings, \
             and any issues that need attention.",
            plan.goal_id,
            summary_parts.join("\n"),
        );

        let system = "You are a Senior Technical Lead. Synthesize multi-agent results concisely.";
        if let Ok(s) = llm.chat(system, &prompt, false).await {
            Some(s)
        } else {
            let success_count = results.iter().filter(|r| r.success).count();
            Some(format!(
                "Completed {} tasks ({} successful, {} failed)",
                results.len(),
                success_count,
                results.len() - success_count,
            ))
        }
    }

    /// Decompose a `TeamGoal` into a `TeamPlan` using the LLM.
    pub async fn decompose_goal(&self, goal: &TeamGoal) -> Result<TeamPlan, TeamError> {
        let llm = self.planner_llm.as_ref().ok_or(TeamError::NoPlanner)?;

        let agents = self.registry.list();
        let agent_descriptions: Vec<String> = agents
            .iter()
            .map(|(id, info)| {
                format!(
                    "- `{}` ({}): {} — tools: {}",
                    id,
                    info.display_name,
                    info.description,
                    info.tool_prefixes.join(", ")
                )
            })
            .collect();

        let system_prompt = format!(
            "You are a senior engineering lead that decomposes complex goals into \
             parallel task DAGs for a team of specialist AI agents.\n\n\
             Available agents:\n{}\n\n\
             Rules:\n\
             1. Break the goal into the smallest meaningful tasks — prefer more granular \
             tasks that can run in parallel.\n\
             2. Assign each task to the most suitable agent.\n\
             3. Set `depends_on` only when a task genuinely needs another's output.\n\
             4. Use `args` to pass the exact JSON arguments the tool expects.\n\
             5. Use `id` values that are short, kebab-case, and unique (e.g. \
             `\"research-competitors\"`, `\"review-architecture\"`).\n\
             6. Include context from the goal description in args where useful.\n\n\
             Respond ONLY with a JSON object in this exact shape (no markdown, no \
             explanation):\n\
             {{\n\
               \"tasks\": [\n\
                 {{\n\
                   \"id\": \"task-id\",\n\
                   \"agent\": \"agent-name\",\n\
                   \"tool_id\": \"agent/tool\",\n\
                   \"args\": {{ /* tool-specific JSON arguments */ }},\n\
                   \"depends_on\": []\n\
                 }}\n\
               ]\n\
             }}",
            agent_descriptions.join("\n"),
        );

        let user_prompt = format!(
            "Goal: {}\n{}",
            goal.objective,
            goal.context
                .as_ref()
                .map(|c| format!("Context: {c}"))
                .unwrap_or_default(),
        );

        let response = llm
            .chat(&system_prompt, &user_prompt, true)
            .await
            .map_err(|e| TeamError::LlmFailed(e.to_string()))?;

        parse_decomposition_response(&response, &goal.id).map_err(TeamError::InvalidPlan)
    }
}
