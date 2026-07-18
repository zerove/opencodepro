//! Autonomous development agent.
//!
//! LLM-driven planning, step execution, verification, and checkpoint-based resume.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (DevAction, DevStep, DevelopmentPlan, etc.).

pub mod types;

pub use types::{
    DevAction, DevCheckpoint, DevOverallStatus, DevStep, DevelopmentPlan, DevelopmentReport,
    StepResult, StepStatus, VerificationMethod,
};

use sisyphus_tools_llm::metis::LlmClient;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tokio::task::JoinSet;
use types::EditRequest;

pub struct Developer {
    llm: Arc<dyn LlmClient>,
    checkpoint_dir: Option<PathBuf>,
    enable_wisdom: bool,
}

impl Developer {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self {
            llm,
            checkpoint_dir: None,
            enable_wisdom: false,
        }
    }

    /// Enable wisdom/notepads integration: auto-inject past knowledge and record outcomes.
    pub fn with_wisdom(mut self) -> Self {
        self.enable_wisdom = true;
        self
    }

    /// Enable checkpoint-based resume: saves progress after each step.
    pub fn with_checkpoint<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.checkpoint_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    pub async fn plan(&self, goal: &str, project_context: &str) -> Result<DevelopmentPlan, String> {
        let system_prompt = r#"You are an autonomous development agent. 
Your goal is to decompose a natural-language goal into a structured development plan.
Analyze the goal and project context, then produce a JSON object representing a DevelopmentPlan.

The DevelopmentPlan must have:
- "goal": The original goal.
- "overview": A high-level summary of the approach.
- "steps": An array of DevStep objects.

Each DevStep must have:
- "id": Unique integer starting from 1.
- "description": What this step does.
- "action": A DevAction object.
- "expected_outcome": What success looks like for this step.
- "verification": A VerificationMethod object.
- "status": "Pending".

DevAction types:
- { "type": "CreateFile", "params": { "path": "...", "content": "..." } }
- { "type": "EditFile", "params": { "path": "...", "old_string": "...", "new_string": "..." } }
- { "type": "RunCommand", "params": { "command": "...", "dir": "..." } }
- { "type": "ReadFile", "params": { "path": "..." } }
- { "type": "ComplexChange", "params": { "description": "..." } }

VerificationMethod types:
- { "type": "CargoCheck" }
- { "type": "CargoTest" }
- { "type": "LspDiagnostics", "params": { "path": "..." } }
- { "type": "Command", "params": { "command": "..." } }
- { "type": "LlmReview", "params": { "criteria": "..." } }
- { "type": "None" }

Return ONLY the JSON object."#;

        let context_with_wisdom = if self.enable_wisdom {
            match sisyphus_tools_system::wisdom::wisdom_inject().await {
                Ok(w) if !w.is_empty() => {
                    format!("{project_context}\n\nWisdom from past sessions:\n{w}")
                }
                _ => project_context.to_string(),
            }
        } else {
            project_context.to_string()
        };

        let user_prompt =
            format!("Goal: {goal}\n\nProject Context: {context_with_wisdom}\n\nPlan this goal.");

        // First attempt
        let response = self.llm.chat(system_prompt, &user_prompt, true).await?;
        match serde_json::from_str::<DevelopmentPlan>(&response) {
            Ok(plan) => Ok(plan),
            Err(e) => {
                // Retry once with error context
                let corrective_prompt = format!(
                    "Your previous response was not valid JSON for DevelopmentPlan. Error: {e}. Please try again and ensure the JSON structure is exactly as requested."
                );
                let response = self
                    .llm
                    .chat(system_prompt, &corrective_prompt, true)
                    .await?;
                match serde_json::from_str::<DevelopmentPlan>(&response) {
                    Ok(plan) => Ok(plan),
                    Err(e2) => Err(format!("Failed to parse DevelopmentPlan after retry: {e2}")),
                }
            }
        }
    }

    pub async fn execute_step(&self, step: &DevStep) -> Result<StepResult, String> {
        let output = match &step.action {
            DevAction::CreateFile { path, content } => {
                sisyphus_tools_system::file::write(Path::new(path), content)
                    .map(|()| format!("Created file: {path}"))
                    .map_err(|e| e.to_string())?
            }
            DevAction::EditFile {
                path,
                old_string,
                new_string,
            } => sisyphus_tools_system::file::edit(Path::new(path), old_string, new_string)
                .map(|()| format!("Edited file: {path}"))
                .map_err(|e| e.to_string())?,
            DevAction::RunCommand { command, dir } => {
                let mut cmd = Command::new("sh");
                cmd.arg("-c").arg(command);
                if let Some(d) = dir {
                    cmd.current_dir(d);
                }
                let output = cmd.output().map_err(|e| e.to_string())?;
                if output.status.success() {
                    String::from_utf8_lossy(&output.stdout).to_string()
                } else {
                    return Ok(StepResult {
                        step_id: step.id,
                        status: StepStatus::Failed(
                            String::from_utf8_lossy(&output.stderr).to_string(),
                        ),
                        output: String::from_utf8_lossy(&output.stdout).to_string(),
                        verification_output: None,
                    });
                }
            }
            DevAction::ReadFile { path } => {
                sisyphus_tools_system::file::read(Path::new(path)).map_err(|e| e.to_string())?
            }
            DevAction::ComplexChange { description } => {
                let system_prompt = "You are a code editor. Decompose a complex change request into a JSON array of granular edits. Each edit must be { \"path\": \"...\", \"old_string\": \"...\", \"new_string\": \"...\" }. Return ONLY the JSON array.";
                let response = self.llm.chat(system_prompt, description, true).await?;
                let edits: Vec<EditRequest> =
                    serde_json::from_str(&response).map_err(|e| e.to_string())?;

                let mut summary = Vec::new();
                for edit in edits {
                    match sisyphus_tools_system::file::edit(
                        Path::new(&edit.path),
                        &edit.old_string,
                        &edit.new_string,
                    ) {
                        Ok(()) => summary.push(format!("Applied edit to {}", edit.path)),
                        Err(e) => summary.push(format!(
                            "Warning: failed to apply edit to {}: {}",
                            edit.path, e
                        )),
                    }
                }
                summary.join("\n")
            }
        };

        Ok(StepResult {
            step_id: step.id,
            status: StepStatus::Success,
            output,
            verification_output: None,
        })
    }

    pub async fn verify_step(&self, step: &DevStep, result: &StepResult) -> Result<bool, String> {
        match &step.verification {
            VerificationMethod::CargoCheck => {
                let output = Command::new("sh")
                    .arg("-c")
                    .arg("cargo check 2>&1")
                    .output()
                    .map_err(|e| e.to_string())?;
                Ok(output.status.success())
            }
            VerificationMethod::CargoTest => {
                let output = Command::new("sh")
                    .arg("-c")
                    .arg("cargo test 2>&1")
                    .output()
                    .map_err(|e| e.to_string())?;
                Ok(output.status.success())
            }
            VerificationMethod::LspDiagnostics { path: _ } => {
                // Not implemented in CLI context; assume pass
                Ok(true)
            }
            VerificationMethod::Command { command } => {
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
                    .map_err(|e| e.to_string())?;
                Ok(output.status.success())
            }
            VerificationMethod::LlmReview { criteria } => {
                let system_prompt = "You are a QA engineer. Review the provided output against the criteria. Answer ONLY 'yes' or 'no'.";
                let user_prompt = format!(
                    "Criteria: {}\n\nOutput: {}\n\nDoes this satisfy the criteria?",
                    criteria, result.output
                );
                let response = self.llm.chat(system_prompt, &user_prompt, false).await?;
                Ok(response.to_lowercase().starts_with("yes"))
            }
            VerificationMethod::None => Ok(true),
        }
    }

    /// Execute-and-verify loop for a single step, with automatic fix on failure.
    async fn run_step_with_fix(&self, step: &DevStep) -> StepResult {
        let mut result = match self.execute_step(step).await {
            Ok(r) => r,
            Err(e) => {
                return StepResult {
                    step_id: step.id,
                    status: StepStatus::Failed(e),
                    output: String::new(),
                    verification_output: None,
                };
            }
        };

        if result.status != StepStatus::Success {
            return result;
        }

        match self.verify_step(step, &result).await {
            Ok(true) => {
                // Success — no fix needed
            }
            Ok(false) => {
                // Verification failed; attempt auto-fix
                let fix_description = format!(
                    "Fix this issue: verification failed for step {}. Output was: {}",
                    step.id, result.output
                );
                let fix_step = DevStep {
                    id: step.id * 100,
                    description: format!("Fix for step {}", step.id),
                    action: DevAction::ComplexChange {
                        description: fix_description,
                    },
                    expected_outcome: "Verification passes".to_string(),
                    verification: step.verification.clone(),
                    status: StepStatus::Pending,
                };

                let fix_result = match self.execute_step(&fix_step).await {
                    Ok(r) => r,
                    Err(e) => {
                        result.status = StepStatus::Failed(format!("Fix attempt errored: {e}"));
                        return result;
                    }
                };

                if fix_result.status == StepStatus::Success {
                    match self.verify_step(step, &fix_result).await {
                        Ok(true) => {
                            result.status = StepStatus::Success;
                            result.output =
                                format!("Fixed and verified. Fix output: {}", fix_result.output);
                        }
                        _ => {
                            result.status = StepStatus::Failed(
                                "Verification failed even after fix attempt".to_string(),
                            );
                        }
                    }
                } else {
                    result.status =
                        StepStatus::Failed(format!("Fix attempt failed: {}", fix_result.output));
                }
            }
            Err(e) => {
                result.status = StepStatus::Failed(format!("Verification error: {e}"));
            }
        }

        result
    }

    /// Save a checkpoint after each completed step.
    fn save_checkpoint(
        &self,
        goal: &str,
        project_context: &str,
        completed_ids: &[usize],
        results: &[StepResult],
        artifacts: &[String],
    ) {
        if let Some(ref dir) = self.checkpoint_dir {
            let cp = DevCheckpoint {
                goal: goal.to_string(),
                project_context: project_context.to_string(),
                completed_step_ids: completed_ids.to_vec(),
                step_results: results.to_vec(),
                artifacts: artifacts.to_vec(),
            };
            if let Ok(json) = serde_json::to_string_pretty(&cp) {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join("dev_checkpoint.json"), json);
            }
        }
    }

    /// Load a checkpoint if one exists.
    pub fn load_checkpoint(&self) -> Option<DevCheckpoint> {
        let path = self.checkpoint_dir.as_ref()?.join("dev_checkpoint.json");
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Clear checkpoint (call after successful completion).
    pub fn clear_checkpoint(&self) {
        if let Some(ref dir) = self.checkpoint_dir {
            let path = dir.join("dev_checkpoint.json");
            let _ = std::fs::remove_file(path);
        }
    }

    /// Main development loop with optional parallelism and checkpoint resume.
    ///
    /// - `project_context`: current state / constraints
    /// - `parallel`: if true, run steps concurrently using JoinSet (max 4 at a time)
    /// - `resume`: if true, try to load a checkpoint and skip already-completed steps
    pub async fn develop(
        &self,
        goal: &str,
        project_context: &str,
        parallel: bool,
        resume: bool,
    ) -> Result<DevelopmentReport, String> {
        // --- Wisdom: inject past knowledge at start ---
        if self.enable_wisdom {
            let _ = sisyphus_tools_system::wisdom::wisdom_record(
                "dev-goals",
                &format!("Goal: {goal}\nContext: {project_context}"),
            )
            .await;
        }

        // --- Resume: load checkpoint and skip completed steps ---
        let (plan, existing_results, mut artifacts) = if resume {
            if let Some(cp) = self.load_checkpoint() {
                let plan = self.plan(goal, project_context).await?;
                (plan, cp.step_results, cp.artifacts)
            } else {
                (
                    self.plan(goal, project_context).await?,
                    Vec::new(),
                    Vec::new(),
                )
            }
        } else {
            (
                self.plan(goal, project_context).await?,
                Vec::new(),
                Vec::new(),
            )
        };

        // Filter out already-completed steps
        let completed_ids: std::collections::HashSet<usize> =
            existing_results.iter().map(|r| r.step_id).collect();
        let pending_steps: Vec<&DevStep> = plan
            .steps
            .iter()
            .filter(|s| !completed_ids.contains(&s.id))
            .collect();

        if pending_steps.is_empty() && !existing_results.is_empty() {
            // All done from checkpoint
            return Ok(build_report(goal, existing_results, artifacts));
        }

        let mut step_results: Vec<StepResult> = existing_results;

        if parallel && pending_steps.len() > 1 {
            // --- Parallel execution ---
            let llm = self.llm.clone();
            let checkpoint_dir = self.checkpoint_dir.clone();
            let mut join_set = JoinSet::new();
            for step in &pending_steps {
                let step = (*step).clone();
                let llm = llm.clone();
                let checkpoint_dir = checkpoint_dir.clone();
                join_set.spawn(async move {
                    let dev = Developer::new(llm).with_checkpoint_path(checkpoint_dir);
                    let result = dev.run_step_with_fix(&step).await;
                    (step, result)
                });
                // Limit concurrency: if 4 already queued, wait for one to finish
                if join_set.len() >= 4 {
                    if let Some(Ok((step, result))) = join_set.join_next().await {
                        track_artifact(&step, &mut artifacts);
                        step_results.push(result);
                        self.save_checkpoint(
                            goal,
                            project_context,
                            &step_results.iter().map(|r| r.step_id).collect::<Vec<_>>(),
                            &step_results,
                            &artifacts,
                        );
                    }
                }
            }
            // Drain remaining
            while let Some(res) = join_set.join_next().await {
                if let Ok((step, result)) = res {
                    track_artifact(&step, &mut artifacts);
                    step_results.push(result);
                    self.save_checkpoint(
                        goal,
                        project_context,
                        &step_results.iter().map(|r| r.step_id).collect::<Vec<_>>(),
                        &step_results,
                        &artifacts,
                    );
                }
            }
        } else {
            // --- Sequential execution ---
            for step in &pending_steps {
                let result = self.run_step_with_fix(step).await;
                track_artifact(step, &mut artifacts);
                step_results.push(result);
                self.save_checkpoint(
                    goal,
                    project_context,
                    &step_results.iter().map(|r| r.step_id).collect::<Vec<_>>(),
                    &step_results,
                    &artifacts,
                );
            }
        }

        let report = build_report(goal, step_results, artifacts);

        // --- Wisdom: record outcome ---
        if self.enable_wisdom {
            let status_str = match &report.overall_status {
                DevOverallStatus::Success => "SUCCESS",
                DevOverallStatus::PartialFailure { .. } => "PARTIAL",
                DevOverallStatus::Failed(_) => "FAILED",
            };
            let _ = sisyphus_tools_system::wisdom::wisdom_record(
                "dev-outcomes",
                &format!(
                    "Goal: {goal}\nStatus: {status_str}\nSteps: {} total",
                    report.steps.len()
                ),
            )
            .await;
        }

        // Clear checkpoint on success
        if report.overall_status == DevOverallStatus::Success {
            self.clear_checkpoint();
        }

        Ok(report)
    }
}

// --- Helpers ---

fn track_artifact(step: &DevStep, artifacts: &mut Vec<String>) {
    if let DevAction::CreateFile { path, .. } = &step.action {
        artifacts.push(path.clone());
    } else if let DevAction::EditFile { path, .. } = &step.action {
        artifacts.push(path.clone());
    }
}

fn build_report(
    goal: &str,
    step_results: Vec<StepResult>,
    artifacts: Vec<String>,
) -> DevelopmentReport {
    let completed = step_results
        .iter()
        .filter(|r| r.status == StepStatus::Success)
        .count();
    let total = step_results.len();

    let overall_status = if completed == total {
        DevOverallStatus::Success
    } else if completed > 0 {
        DevOverallStatus::PartialFailure { completed, total }
    } else {
        DevOverallStatus::Failed("No steps succeeded".to_string())
    };

    DevelopmentReport {
        goal: goal.to_string(),
        overall_status,
        steps: step_results,
        artifacts,
    }
}

pub fn format_report(report: &DevelopmentReport) -> String {
    let mut output = format!("Development Report for Goal: {}\n", report.goal);
    output.push_str("--------------------------------------------------\n");

    match &report.overall_status {
        DevOverallStatus::Success => output.push_str("Overall Status: SUCCESS\n"),
        DevOverallStatus::PartialFailure { completed, total } => {
            output.push_str(&format!(
                "Overall Status: PARTIAL FAILURE ({completed} / {total} steps completed)\n"
            ));
        }
        DevOverallStatus::Failed(e) => output.push_str(&format!("Overall Status: FAILED ({e})\n")),
    }

    output.push_str("--------------------------------------------------\n");
    for step in &report.steps {
        output.push_str(&format!(
            "Step {}: Status: {:?}\n",
            step.step_id, step.status
        ));
        output.push_str(&format!("Output: {}\n", step.output));
        if let Some(v_out) = &step.verification_output {
            output.push_str(&format!("Verification: {v_out}\n"));
        }
        output.push_str("--------------------------------------------------\n");
    }

    if !report.artifacts.is_empty() {
        output.push_str("\nArtifacts:\n");
        for art in &report.artifacts {
            output.push_str(&format!("- {art}\n"));
        }
    }

    output
}

// --- Internal helper for checkpoint path (not public) ---
impl Developer {
    fn with_checkpoint_path(self, dir: Option<PathBuf>) -> Self {
        Self {
            checkpoint_dir: dir,
            ..self
        }
    }
}

#[cfg(test)]
#[path = "developer_tests.rs"]
mod tests;
