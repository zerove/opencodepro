//! Autonomous development agent.
//!
//! LLM-driven planning, step execution, verification, and checkpoint-based resume.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (DevAction, DevStep, DevelopmentPlan, etc.).
//! - `plan_parsing`: 宽松的计划解析兜底（弱模型经常漏字段/用错变体）。

pub mod plan_parsing;
pub mod types;

pub use types::{
    DevAction, DevCheckpoint, DevOverallStatus, DevStep, DevelopmentPlan, DevelopmentReport,
    FinalVerification, StepResult, StepStatus, VerificationMethod,
};

use sisyphus_tools_llm::metis::LlmClient;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::task::JoinSet;
use types::EditRequest;

use async_trait::async_trait;
use serde_json::Value;

/// 审批闸点（T2.1 MVP）：工具执行前询问用户是否放行。
///
/// MVP 阶段由 TUI 侧注入（审批待确认态 → `approve` 放行/拒绝）；
/// `approve` 内部应 fail-safe（异常/未接待视为拒绝）。
#[async_trait]
pub trait DevApprover: Send + Sync {
    /// 返回 `true` = 放行 / `false` = 拒绝。
    async fn approve(&self, tool_id: &str, _action: &Value) -> Result<bool, String>;
}

/// Developer.
pub struct Developer {
    llm: Arc<dyn LlmClient>,
    checkpoint_dir: Option<PathBuf>,
    enable_wisdom: bool,
    interrupt: Arc<AtomicBool>,
    approver: Option<Arc<dyn DevApprover>>,
    /// 客观验收命令（`dev --verify-cmd`）。
    ///
    /// 步骤的 `verification` 由**模型自选**，可能与用户目标不一致 —— 例如目标是
    /// 「clippy -D warnings 通过」，模型却声明 `CargoCheck`（警告不是错误 → 通过），
    /// 于是「全部步骤成功」而目标其实没达成。这里提供一条用户给定的命令作为最终闸门：
    /// 未通过则带着输出重试有界次数，仍不通过则整体状态判 Failed。
    final_verification: Option<String>,
    /// 最终闸门的自动修复尝试次数（默认 2）。
    final_fix_attempts: usize,
}

impl Developer {
    /// New.
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self {
            llm,
            checkpoint_dir: None,
            enable_wisdom: false,
            interrupt: Arc::new(AtomicBool::new(false)),
            approver: None,
            final_verification: None,
            final_fix_attempts: 2,
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

    /// Enable cancellation: setting the flag interrupts the run at a step boundary.
    ///
    /// MVP（T2.1）：中断即「前端设置 flag → 循环在步骤边界检查 → 保存 checkpoint → 返回 Cancelled」。
    /// 恢复仍复用 [`Developer::develop`] 的 `resume` 参数（checkpoint 加载 + 跳过已完成 step）。
    pub fn with_interrupt(mut self, flag: Arc<AtomicBool>) -> Self {
        self.interrupt = flag;
        self
    }

    /// Enable the approval gate: tool execution asks the approver before running.
    pub fn with_approval_handler(mut self, approver: Arc<dyn DevApprover>) -> Self {
        self.approver = Some(approver);
        self
    }

    /// 设置客观验收命令（最终闸门），例如 `cargo clippy -- -D warnings`。
    ///
    /// 步骤的 `verification` 由模型自选，可能与用户目标不一致；该命令是唯一
    /// 客观判据：未通过则整体判 [`DevOverallStatus::Failed`]。
    pub fn with_final_verification(mut self, command: impl Into<String>) -> Self {
        self.final_verification = Some(command.into());
        self
    }

    /// 最终闸门失败后的自动修复尝试次数（默认 2，0 表示只判定不修复）。
    pub fn with_final_fix_attempts(mut self, attempts: usize) -> Self {
        self.final_fix_attempts = attempts;
        self
    }

    /// Approval gate for a step action (T2.1 MVP).
    ///
    /// Returns `Ok(true)` to execute, `Ok(false)` to deny. LLM-internal
    /// actions (`ComplexChange`) bypass the gate in MVP.
    async fn approve_step(&self, step: &DevStep) -> Result<bool, String> {
        let Some(approver) = self.approver.as_ref() else {
            return Ok(true);
        };
        let Some(tool_id) = dev_tool_id(&step.action) else {
            return Ok(true);
        };
        let action_json = serde_json::to_value(&step.action).map_err(|e| e.to_string())?;
        let allowed = approver.approve(tool_id, &action_json).await.map_err(|e| e.to_string())?;
        Ok(allowed)
    }

    /// Plan.
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
                },
                _ => project_context.to_string(),
            }
        } else {
            project_context.to_string()
        };

        // 目标**前后都放**：
        // - 放前面：保持既有行为（弱模型对「第一句是目标」友好），短上下文场景
        //   不受影响；
        // - 放后面：provider 截断超长 prompt 时保留的是尾部（本地 ollama 实测
        //   从头部裁剪），目标写在最前面会被一起裁掉 —— 实测长上下文场景里
        //   模型因此把任务误解成「实现 helper_320」。
        //
        // 末尾再补一段**精简 schema 提醒**：长上下文时 system prompt 里的完整
        // schema 也会被裁掉，模型于是只回一句摘要而不是计划（实测）。
        let user_prompt = format!(
            "Goal: {goal}\n\nProject Context: {context_with_wisdom}\n\n\
Goal (repeat): {goal}\n\n{PLAN_SCHEMA_REMINDER}\n\nPlan this goal."
        );

        // First attempt
        let response =
            self.llm.chat(system_prompt, &user_prompt, true).await.map_err(|e| e.to_string())?;
        match serde_json::from_str::<DevelopmentPlan>(&response) {
            Ok(plan) => Ok(plan),
            Err(strict_err) => {
                // 先试宽松解析：漏字段/未知验证类型不该让整份计划作废
                // （实测弱模型经常缺 `goal`，或把动作类型当 verification）。
                match plan_parsing::parse_plan_lenient(&response, goal) {
                    Ok(plan) => {
                        tracing::warn!(
                            target: "dev",
                            error = %strict_err,
                            "计划未严格匹配 schema，已用宽松解析兜底"
                        );
                        return Ok(plan);
                    },
                    Err(lenient_err) => {
                        tracing::warn!(
                            target: "dev",
                            strict_error = %strict_err,
                            lenient_error = %lenient_err,
                            "严格与宽松解析都失败"
                        );
                    },
                }
                // 重试必须带上**原始任务**：只发报错信息时，弱模型会彻底丢失目标
                // （实测 3B 模型把「拆 add 到 src/math.rs」重试成「新建 Hello World 项目」，
                // 并真的执行了那个计划，覆盖掉用户仓库的 Cargo.toml）。
                let corrective_prompt = format!(
                    "{user_prompt}\n\n---\nYour previous response was not valid JSON for DevelopmentPlan.\nError: {strict_err}\nPrevious response (truncated): {}\n\nFix ONLY the schema error. `verification.type` must be exactly one of: CargoCheck, CargoTest, LspDiagnostics, Command, LlmReview, None. Action types (CreateFile / EditFile / RunCommand / ReadFile / ComplexChange) are NOT verification types.\nReturn ONLY the corrected JSON object for the SAME goal and project context above.",
                    truncate_chars(&response, 800)
                );
                let response = self
                    .llm
                    .chat(system_prompt, &corrective_prompt, true)
                    .await
                    .map_err(|e| e.to_string())?;
                match serde_json::from_str::<DevelopmentPlan>(&response) {
                    Ok(plan) => Ok(plan),
                    Err(e2) => match plan_parsing::parse_plan_lenient(&response, goal) {
                        Ok(plan) => {
                            tracing::warn!(
                                target: "dev",
                                error = %e2,
                                "重试后的计划未严格匹配 schema，已用宽松解析兜底"
                            );
                            Ok(plan)
                        },
                        Err(_) => Err(format!("Failed to parse DevelopmentPlan after retry: {e2}")),
                    },
                }
            },
        }
    }

    /// Execute step.
    pub async fn execute_step(&self, step: &DevStep) -> Result<StepResult, String> {
        let output = match &step.action {
            DevAction::CreateFile { path, content } => {
                // 覆盖已有文件不是错误（弱模型经常用 CreateFile 表达「整文件重写」，
                // 精确 Edit 对它们太难），但必须**如实标注**：静默覆盖会毁掉用户
                // 仓库里的文件（实测模型把 Cargo.toml 覆盖成了 hello_world 项目）。
                let existed = std::path::Path::new(path).is_file();
                let previous_len = if existed {
                    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
                } else {
                    0
                };
                sisyphus_tools_system::file::write(Path::new(path), content)
                    .map(|()| {
                        if existed {
                            format!("Overwrote existing file: {path} (was {previous_len} bytes)")
                        } else {
                            format!("Created file: {path}")
                        }
                    })
                    .map_err(|e| e.to_string())?
            },
            DevAction::EditFile {
                path,
                old_string,
                new_string,
            } => sisyphus_tools_system::file::edit(Path::new(path), old_string, new_string)
                .map(|()| format!("Edited file: {path}"))
                .map_err(|e| e.to_string())?,
            DevAction::RunCommand { command, dir } => {
                let mut opts = sisyphus_tools_core::process::CommandOpts::default();
                if let Some(d) = dir {
                    opts = opts.with_workdir(d);
                }
                let output =
                    sisyphus_tools_core::process::run_command_blocking("sh", ["-c", command], opts)
                        .map_err(|e| e.to_string())?;
                if output.exit_code == 0 {
                    output.stdout.clone()
                } else {
                    return Ok(StepResult {
                        step_id: step.id,
                        status: StepStatus::Failed(output.stderr.clone()),
                        output: output.stdout.clone(),
                        verification_output: None,
                    });
                }
            },
            DevAction::ReadFile { path } => {
                sisyphus_tools_system::file::read(Path::new(path)).map_err(|e| e.to_string())?
            },
            DevAction::ComplexChange { description } => {
                let system_prompt = "You are a code editor. Decompose a complex change request into a JSON array of granular edits. Each edit must be { \"path\": \"...\", \"old_string\": \"...\", \"new_string\": \"...\" }. Return ONLY the JSON array.";
                let response = self
                    .llm
                    .chat(system_prompt, description, true)
                    .await
                    .map_err(|e| e.to_string())?;
                let edits = parse_edit_requests(&response)?;

                if edits.is_empty() {
                    return Err("complex change produced no edits".to_string());
                }
                // 编辑失败不能吞成 warning + Success：上层会把「没改成」当成
                // 已完成并跳过修复（与「成功标准由模型自选」是同一类缺陷）。
                let mut summary = Vec::new();
                let mut failures = Vec::new();
                for edit in edits {
                    match sisyphus_tools_system::file::edit(
                        Path::new(&edit.path),
                        &edit.old_string,
                        &edit.new_string,
                    ) {
                        Ok(()) => summary.push(format!("Applied edit to {}", edit.path)),
                        Err(e) => failures.push(format!("{}: {e}", edit.path)),
                    }
                }
                if !failures.is_empty() {
                    return Err(format!(
                        "failed to apply {}/{} edit(s): {}",
                        failures.len(),
                        summary.len() + failures.len(),
                        failures.join("; ")
                    ));
                }
                summary.join("\n")
            },
        };

        Ok(StepResult {
            step_id: step.id,
            status: StepStatus::Success,
            output,
            verification_output: None,
        })
    }

    /// Verify step.
    /// 运行验证命令并返回 `(是否通过, 合并输出)`。
    ///
    /// 最终闸门需要把失败输出喂给修复环节，因此不能只返回 bool。
    fn run_verification_output(command: &str) -> Result<(bool, String), String> {
        let out = sisyphus_tools_core::process::run_command_blocking(
            "sh",
            ["-c", command],
            sisyphus_tools_core::process::CommandOpts::default(),
        )
        .map_err(|e| e.to_string())?;
        let mut combined = out.stdout.clone();
        if !out.stderr.is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&out.stderr);
        }
        Ok((out.exit_code == 0, combined))
    }

    /// Run a verification command and report whether it exited cleanly.
    fn run_verification(command: &str) -> Result<bool, String> {
        let out = sisyphus_tools_core::process::run_command_blocking(
            "sh",
            ["-c", command],
            sisyphus_tools_core::process::CommandOpts::default(),
        )
        .map_err(|e| e.to_string())?;
        Ok(out.exit_code == 0)
    }

    pub async fn verify_step(&self, step: &DevStep, result: &StepResult) -> Result<bool, String> {
        match &step.verification {
            VerificationMethod::CargoCheck => Ok(Self::run_verification("cargo check")?),
            VerificationMethod::CargoTest => Ok(Self::run_verification("cargo test")?),
            VerificationMethod::LspDiagnostics { path: _ } => {
                // Not implemented in CLI context; assume pass
                Ok(true)
            },
            VerificationMethod::Command { command } => Ok(Self::run_verification(command)?),
            VerificationMethod::LlmReview { criteria } => {
                let system_prompt = "You are a QA engineer. Review the provided output against the criteria. Answer ONLY 'yes' or 'no'.";
                let user_prompt = format!(
                    "Criteria: {}\n\nOutput: {}\n\nDoes this satisfy the criteria?",
                    criteria, result.output
                );
                let response = self
                    .llm
                    .chat(system_prompt, &user_prompt, false)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(response.to_lowercase().starts_with("yes"))
            },
            VerificationMethod::None => Ok(true),
        }
    }

    /// Execute-and-verify loop for a single step, with automatic fix on failure.
    async fn run_step_with_fix(&self, step: &DevStep) -> StepResult {
        let mut result = match self.execute_step(step).await {
            Ok(r) => r,
            Err(e) => {
                // 执行失败（编辑匹配不上、命令跑不起来…）也交给模型纠错一次 ——
                // 旧实现直接失败返回，而这类错误信息恰恰是模型最需要的上下文
                // （实测 `EditFile` 的 old_string 匹配不上时，模型自己能改对）。
                let fix_step = DevStep {
                    id: step.id * 100,
                    description: format!("Fix for step {}", step.id),
                    action: DevAction::ComplexChange {
                        description: format!(
                            "Step {} ({}) failed: {}. Perform the intended change with exact-match edits.",
                            step.id,
                            step.description,
                            e
                        ),
                    },
                    expected_outcome: "The intended change is applied".to_string(),
                    verification: step.verification.clone(),
                    status: StepStatus::Pending,
                };
                let fix_result = match self.execute_step(&fix_step).await {
                    Ok(r) => r,
                    Err(fix_err) => {
                        return StepResult {
                            step_id: step.id,
                            status: StepStatus::Failed(format!(
                                "{e} (fix attempt errored: {fix_err})"
                            )),
                            output: String::new(),
                            verification_output: None,
                        };
                    },
                };
                if fix_result.status != StepStatus::Success {
                    return StepResult {
                        step_id: step.id,
                        status: StepStatus::Failed(format!("{e} (fix attempt failed)")),
                        output: fix_result.output,
                        verification_output: None,
                    };
                }
                return StepResult {
                    step_id: step.id,
                    status: StepStatus::Success,
                    output: format!("Fixed after execution error ({e}). {}", fix_result.output),
                    verification_output: None,
                };
            },
        };

        if result.status != StepStatus::Success {
            return result;
        }

        match self.verify_step(step, &result).await {
            Ok(true) => {
                // Success — no fix needed
            },
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
                    },
                };

                if fix_result.status == StepStatus::Success {
                    match self.verify_step(step, &fix_result).await {
                        Ok(true) => {
                            result.status = StepStatus::Success;
                            result.output =
                                format!("Fixed and verified. Fix output: {}", fix_result.output);
                        },
                        _ => {
                            result.status = StepStatus::Failed(
                                "Verification failed even after fix attempt".to_string(),
                            );
                        },
                    }
                } else {
                    result.status =
                        StepStatus::Failed(format!("Fix attempt failed: {}", fix_result.output));
                }
            },
            Err(e) => {
                result.status = StepStatus::Failed(format!("Verification error: {e}"));
            },
        }

        result
    }

    /// 让 LLM 修复文件，接受两种输出形态（优先整文件重写）。
    ///
    /// 本地小模型很难精确复现 `old_string`（换行/缩进一错就 no match），而「输出
    /// 完整文件」对它更自然。因此闸门修复同时接受 `{"files":[{path,content}]}`
    /// 与 `[{path,old_string,new_string}]`，前者优先。
    async fn repair_files(
        &self,
        prompt: &str,
        verification_output: &str,
    ) -> Result<String, String> {
        const SYSTEM: &str = "You are a code fixer. Return ONLY JSON. Preferred: {\"files\":[{\"path\":\"...\",\"content\":\"<complete corrected file>\"}]}. Alternative: [{\"path\":\"...\",\"old_string\":\"...\",\"new_string\":\"...\"}].";
        let response = self.llm.chat(SYSTEM, prompt, true).await.map_err(|e| e.to_string())?;
        apply_repair_response(&response, verification_output)
    }

    /// 运行客观验收门（用户通过 `--verify-cmd` 指定），失败时带着输出做有界自动修复。
    ///
    /// 与 `run_step_with_fix` 的区别：那里失败重试的是**模型自选的** `verification`，
    /// 这里重试的是**用户给定的**命令。只有后者能回答「目标达成了吗」。
    ///
    /// 返回 `None` 表示没有配置验收门（行为与旧版一致）。
    async fn run_final_verification(
        &self,
        goal: &str,
        steps: &mut Vec<StepResult>,
    ) -> Option<FinalVerification> {
        let command = self.final_verification.clone()?;
        let mut fix_attempts = 0usize;
        let mut initial_passed: Option<bool> = None;

        loop {
            let (passed, output) = match Self::run_verification_output(&command) {
                Ok(v) => v,
                // 命令跑不起来（例如工作目录里没有 Cargo.toml）也是「未通过」，
                // 但绝不能当成通过 —— 否则闸门形同虚设。
                Err(e) => (false, format!("failed to run `{command}`: {e}")),
            };
            if initial_passed.is_none() {
                initial_passed = Some(passed);
            }

            if passed || fix_attempts >= self.final_fix_attempts {
                return Some(FinalVerification {
                    command,
                    initial_passed: initial_passed.unwrap_or(passed),
                    passed,
                    fix_attempts,
                    output: truncate_chars(&output, 4000),
                });
            }

            fix_attempts += 1;
            let file_context = gate_file_context(&output);
            let change_diff = gate_change_diff();
            let prompt = format!(
                "The objective verification command failed and MUST pass.\n\
                 Goal: {goal}\n\
                 Verification command: `{command}`\n\
                 Verification output:\n{output}\n\
                 {change_diff}\
                 {file_context}\n\
                 Do NOT regress unrelated behavior: keep existing `pub` visibility and public API.\n\
                 Return ONLY JSON in ONE of these forms:\n\
                 1) {{\"files\":[{{\"path\":\"src/lib.rs\",\"content\":\"<COMPLETE corrected file content>\"}}]}}\n\
                 2) [{{\"path\":\"src/lib.rs\",\"old_string\":\"<exact current text>\",\"new_string\":\"<replacement>\"}}]\n\
                 Prefer form 1 for small files. `content` must be the ENTIRE file with no omissions.\n\
                 For form 2, `old_string` must be copied verbatim from the current content, preserving newlines and indentation."
            );
            let (status, repair_output) = match self.repair_files(&prompt, &output).await {
                Ok(summary) => (StepStatus::Success, summary),
                // 修复环节自身失败（JSON 解析失败 / old_string 不匹配）不是「已修复」，
                // 必须如实记录；循环仍会再跑一次命令，用实际状态决定下一步。
                Err(e) => (StepStatus::Failed(e), String::new()),
            };
            steps.push(StepResult {
                step_id: 9000 + fix_attempts,
                status,
                output: repair_output,
                verification_output: Some(truncate_chars(&output, 2000)),
            });
        }
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
        let pending_steps: Vec<&DevStep> =
            plan.steps.iter().filter(|s| !completed_ids.contains(&s.id)).collect();

        if pending_steps.is_empty() && !existing_results.is_empty() {
            // All done from checkpoint
            return Ok(build_report(goal, existing_results, artifacts, None));
        }

        let mut step_results: Vec<StepResult> = existing_results;
        let mut cancelled = false;

        if parallel && pending_steps.len() > 1 {
            // --- Parallel execution ---
            let llm = self.llm.clone();
            let checkpoint_dir = self.checkpoint_dir.clone();
            let mut join_set = JoinSet::new();
            for step in &pending_steps {
                if self.interrupt.load(Ordering::SeqCst) {
                    cancelled = true;
                    break;
                }
                if !self.approve_step(step).await? {
                    step_results.push(StepResult {
                        step_id: step.id,
                        status: StepStatus::Failed(dev_denied_message(&step.action)),
                        output: String::new(),
                        verification_output: None,
                    });
                    self.save_checkpoint(
                        goal,
                        project_context,
                        &step_results.iter().map(|r| r.step_id).collect::<Vec<_>>(),
                        &step_results,
                        &artifacts,
                    );
                    continue;
                }
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
                if self.interrupt.load(Ordering::SeqCst) {
                    cancelled = true;
                    break;
                }
                if !self.approve_step(step).await? {
                    step_results.push(StepResult {
                        step_id: step.id,
                        status: StepStatus::Failed(dev_denied_message(&step.action)),
                        output: String::new(),
                        verification_output: None,
                    });
                    self.save_checkpoint(
                        goal,
                        project_context,
                        &step_results.iter().map(|r| r.step_id).collect::<Vec<_>>(),
                        &step_results,
                        &artifacts,
                    );
                    continue;
                }
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

        // --- 最终客观验收门（用户给定命令，优先于模型自选的 verification）---
        let final_verification = if cancelled {
            None
        } else {
            self.run_final_verification(goal, &mut step_results).await
        };

        let mut report = build_report(goal, step_results, artifacts, final_verification);
        if cancelled {
            report.overall_status = DevOverallStatus::Cancelled;
        }

        // --- Wisdom: record outcome ---
        if self.enable_wisdom {
            let status_str = match &report.overall_status {
                DevOverallStatus::Success => "SUCCESS",
                DevOverallStatus::PartialFailure { .. } => "PARTIAL",
                DevOverallStatus::Failed(_) => "FAILED",
                DevOverallStatus::Cancelled => "CANCELLED",
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

/// 计划请求末尾的精简 schema 提醒。
///
/// 放在尾部：长上下文被 provider 裁剪时（保留尾部），system prompt 里的完整
/// schema 会连同开头一起被裁掉，模型于是只回一句摘要而不是计划（实测）。
///
/// 刻意用**描述**而不是可照抄的示例模板 —— 带占位值的 JSON 样例会被弱模型
/// 直接抄进输出（实测它曾把 `"path": "..."` 原样写进计划）。
const PLAN_SCHEMA_REMINDER: &str =
    "Return ONLY one JSON object, no markdown and no extra keys, with fields: \
goal (string), overview (string), steps (array). Each step has: id (number), description (string), \
action (object with type + params), expected_outcome (string), verification (object with type), \
status (string \"Pending\"). action.type must be exactly one of: CreateFile, EditFile, RunCommand, ReadFile, ComplexChange.";

/// Map a dev step action to the external tool id it corresponds to (MVP).
///
/// 每个动作都有对应工具 id —— 审批闸按 id 判定，`None` 会让动作**整个跳过**
/// 审批（deny 规则拦不住）。`ComplexChange` 做的是批量文件编辑，映射到
/// `builtin:file_edit`。
fn dev_tool_id(action: &DevAction) -> Option<&'static str> {
    match action {
        DevAction::RunCommand { .. } => Some("builtin:bash_exec"),
        DevAction::CreateFile { .. } => Some("builtin:file_write"),
        DevAction::EditFile { .. } => Some("builtin:file_edit"),
        DevAction::ReadFile { .. } => Some("builtin:file_read"),
        // ComplexChange 实际做的是「批量文件编辑」，映射到 file_edit。
        // 此前返回 None → **整个审批闸被跳过**：deny 规则拦不住它。
        // 映射后它与普通编辑走同一套规则（默认 allow，deny 规则可拦）。
        DevAction::ComplexChange { .. } => Some("builtin:file_edit"),
    }
}

/// Human-readable denial message for an action denied by the user (T2.1 MVP).
fn dev_denied_message(action: &DevAction) -> String {
    dev_tool_id(action)
        .map(|tool_id| format!("denied by user: {tool_id}"))
        .unwrap_or_else(|| "denied by user: unknown action".to_string())
}

/// 按字符截断（避免在多字节 UTF-8 边界上 panic）。
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("\n...[truncated]");
    out
}

/// 收集工作区相对 HEAD 的改动（若在 git 仓库内），供闸门修复参考。
///
/// 实测失败模式：3B 模型把 `pub fn` 改成私有函数触发 `dead_code`，修复环节
/// 只看当前文件，看不到自己引入的回归，于是一遍遍输出同样的私有版本。
/// diff 让它看到「这一步改了什么」，才有机会撤销回归。
fn gate_change_diff() -> String {
    let out = sisyphus_tools_core::process::run_command_blocking(
        "git",
        ["diff", "--no-color", "--", "."],
        sisyphus_tools_core::process::CommandOpts::default(),
    );
    match out {
        Ok(o) if o.exit_code == 0 && !o.stdout.trim().is_empty() => {
            format!(
                "Changes made so far (git diff):\n{}\n",
                truncate_chars(&o.stdout, 3000)
            )
        },
        _ => String::new(),
    }
}

/// 工作区里已改动或新增的文件（`git status --porcelain`）。
///
/// 修复环节最常重写的就是这些文件；若不把它们的**当前内容**给模型，弱模型
/// 会凭记忆「重建」整份文件并丢掉不相关的代码。实测：3B 模型重写 `src/lib.rs`
/// 时丢掉 `format_sum`，闸门因此永远无法通过。
fn changed_paths() -> Vec<String> {
    let out = sisyphus_tools_core::process::run_command_blocking(
        "git",
        ["status", "--porcelain", "--", "."],
        sisyphus_tools_core::process::CommandOpts::default(),
    );
    let Ok(out) = out else {
        return Vec::new();
    };
    if out.exit_code != 0 {
        return Vec::new();
    }

    let mut paths = Vec::new();
    for line in out.stdout.lines() {
        // porcelain 格式：`XY path` 或 `XY old -> new`
        let Some(raw) = line.get(3..) else { continue };
        let path = raw.rsplit(" -> ").next().unwrap_or(raw).trim().trim_matches('"');
        if !(path.ends_with(".rs") || path.ends_with(".toml")) {
            continue;
        }
        if !std::path::Path::new(path).is_file() {
            continue;
        }
        if !paths.iter().any(|p| p == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

/// 从（clippy/rustc 的）验证输出里提取被点名的源文件并读回内容。
///
/// 弱模型在修复环节没有工具可用，只能靠 prompt 里的上下文；只给错误行不够，
/// 它需要看到 `old_string` 要匹配的真实文本。这里按 `--> src/lib.rs:2:8`
/// 之类的形式收集路径（也接受裸路径），再补上工作区已改动/新增的文件
/// （重写这些文件时同样需要看到当前内容），合计最多 4 个文件。
fn gate_file_context(output: &str) -> String {
    let mut paths: Vec<String> = Vec::new();
    for token in output.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-'
        });
        let path = cleaned.split(':').next().unwrap_or(cleaned);
        if !(path.ends_with(".rs") || path.ends_with(".toml")) {
            continue;
        }
        if !std::path::Path::new(path).is_file() {
            continue;
        }
        if paths.iter().any(|p| p == path) {
            continue;
        }
        paths.push(path.to_string());
        if paths.len() >= 3 {
            break;
        }
    }
    for path in changed_paths() {
        if paths.len() >= 4 {
            break;
        }
        if !paths.iter().any(|p| p == &path) {
            paths.push(path);
        }
    }

    if paths.is_empty() {
        return String::new();
    }

    let mut ctx = String::from(
        "Current file contents (preserve unrelated code; old_string must match exactly):\n",
    );
    for p in &paths {
        if let Ok(content) = std::fs::read_to_string(p) {
            ctx.push_str(&format!(
                "--- {p} ---\n{}\n",
                truncate_chars(&content, 3000)
            ));
        }
    }
    ctx
}

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
    final_verification: Option<FinalVerification>,
) -> DevelopmentReport {
    let completed = step_results.iter().filter(|r| r.status == StepStatus::Success).count();
    let total = step_results.len();

    // 配置了验收门时，「用户命令是否通过」是唯一的**目标**判据：
    // - 未通过 → Failed，无论步骤自报多么成功（S2 的真实缺陷）；
    // - 通过但仍有步骤失败 → PartialFailure（目标达成，但过程有失败，需如实展示）；
    // - 通过且步骤全成功 → Success。
    let overall_status = match &final_verification {
        Some(gate) if !gate.passed => DevOverallStatus::Failed(format!(
            "objective verification failed after {} fix attempt(s): `{}`",
            gate.fix_attempts, gate.command
        )),
        Some(_) if completed == total => DevOverallStatus::Success,
        Some(_) => DevOverallStatus::PartialFailure { completed, total },
        None if completed == total => DevOverallStatus::Success,
        None if completed > 0 => DevOverallStatus::PartialFailure { completed, total },
        None => DevOverallStatus::Failed("No steps succeeded".to_string()),
    };

    DevelopmentReport {
        goal: goal.to_string(),
        overall_status,
        steps: step_results,
        artifacts,
        final_verification,
    }
}

/// Format report.
pub fn format_report(report: &DevelopmentReport) -> String {
    let mut output = format!("Development Report for Goal: {}\n", report.goal);
    output.push_str("--------------------------------------------------\n");

    match &report.overall_status {
        DevOverallStatus::Success => output.push_str("Overall Status: SUCCESS\n"),
        DevOverallStatus::PartialFailure { completed, total } => {
            output.push_str(&format!(
                "Overall Status: PARTIAL FAILURE ({completed} / {total} steps completed)\n"
            ));
        },
        DevOverallStatus::Failed(e) => output.push_str(&format!("Overall Status: FAILED ({e})\n")),
        DevOverallStatus::Cancelled => output.push_str("Overall Status: CANCELLED\n"),
    }

    if let Some(gate) = &report.final_verification {
        output.push_str(&format!(
            "Objective verification: {} (`{}`, {} fix attempt(s))\n",
            if gate.passed { "PASSED" } else { "FAILED" },
            gate.command,
            gate.fix_attempts
        ));
        if !gate.passed {
            output.push_str(&format!("Verification output:\n{}\n", gate.output));
        }
        output.push_str("--------------------------------------------------\n");
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

/// 解析「复杂变更」的编辑列表。
///
/// 理想输入是裸数组 `[{path, old_string, new_string}, ...]`，但弱模型（本地 3B/8B）
/// 常给成：
/// - 单个对象 `{...}`（把数组当元素）；
/// - 包裹层 `{"edits": [...]}` / `{"changes": [...]}`；
/// - 前后带 ```json 代码围栏。
///
/// 这些都不该让一次修复直接失败 —— 解析层接受它们，语义不变。
fn parse_edit_requests(raw: &str) -> Result<Vec<EditRequest>, String> {
    let value = parse_json_payload(raw)?;

    let array = match value {
        serde_json::Value::Array(items) => items,
        obj @ serde_json::Value::Object(_) => {
            // 包裹层：取第一个数组值（edits/changes/...）
            let nested = obj.as_object().and_then(|m| m.values().find(|v| v.is_array()).cloned());
            match nested {
                Some(serde_json::Value::Array(items)) => items,
                // 无数组值：按单个 edit 处理
                _ => vec![obj],
            }
        },
        _ => return Err("expected a JSON array or object of edits".to_string()),
    };

    array
        .into_iter()
        .map(|item| {
            serde_json::from_value::<EditRequest>(item).map_err(|e| format!("invalid edit: {e}"))
        })
        .collect()
}

/// 去掉 ```json 围栏并解析 JSON。
fn parse_json_payload(raw: &str) -> Result<serde_json::Value, String> {
    let text = raw.trim();
    let text =
        text.strip_prefix("```json").or_else(|| text.strip_prefix("```")).map_or(text, |s| s);
    let text = text.strip_suffix("```").unwrap_or(text).trim();
    serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))
}

/// `{"path":..., "content":...}` —— 整文件重写请求。
#[derive(serde::Deserialize)]
struct FileRewrite {
    path: String,
    content: String,
}

/// 从响应中提取整文件重写请求（接受数组 / `{"files":[...]}` / 单个对象）。
fn collect_rewrites(value: &serde_json::Value) -> Vec<FileRewrite> {
    let items: Vec<&serde_json::Value> = match value {
        serde_json::Value::Array(items) => items.iter().collect(),
        serde_json::Value::Object(map) => match map.get("files") {
            Some(serde_json::Value::Array(items)) => items.iter().collect(),
            _ => vec![value],
        },
        _ => Vec::new(),
    };

    items
        .into_iter()
        .filter_map(|item| serde_json::from_value::<FileRewrite>(item.clone()).ok())
        .collect()
}

/// 去掉模型包在**文件内容外面**的 markdown 围栏（```rust ... ```）。
///
/// 弱模型经常把「完整文件内容」也套进代码围栏；若原样落盘，围栏会变成源码，
/// 编译必然失败（S2 实测：`error: expected one of ! or ::, found keyword pub`）。
/// 只有首尾都是围栏时才剥离，避免破坏本来就以 ``` 开头的合法文件。
fn strip_code_fence(content: &str) -> String {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") || !trimmed.ends_with("```") || trimmed.chars().count() < 6 {
        return content.to_string();
    }
    let rest = &trimmed[3..];
    let inner = match rest.find('\n') {
        Some(idx) => &rest[idx + 1..],
        None => rest,
    };
    let inner = inner.strip_suffix("```").unwrap_or(inner);
    format!("{}\n", inner.trim_end())
}

/// 提取一段内容里的**公开 API 声明行**（`pub fn` / `pub struct` / …）。
///
/// 用于「修复不得删除公开 API」的约束：闸门修复的目标是让命令通过，
/// 顺手删掉用户已有的公开函数不是目标（实测 3B 修复时把 `pub fn add` 删了，
/// 而 `cargo test` 依然通过 —— 闸门只检查被测行为）。
pub fn public_api_signatures(content: &str) -> Vec<String> {
    content
        .lines()
        .map(|line| line.trim())
        .filter(|line| {
            [
                "pub fn ",
                "pub struct ",
                "pub enum ",
                "pub trait ",
                "pub const ",
                "pub static ",
                "pub type ",
                "pub mod ",
                "pub union ",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
        .map(|line| {
            // 只取 `{` 之前的**签名**部分：函数体重新排版（单行改多行）不算
            // API 变更，否则修复只要重排格式就会被误判成删了公开 API。
            line.split('{').next().unwrap_or(line).trim().to_string()
        })
        .collect()
}

/// 比较新旧内容，返回**被删除**的公开 API 声明。
pub fn deleted_public_apis(old_content: &str, new_content: &str) -> Vec<String> {
    let new_set = public_api_signatures(new_content);
    public_api_signatures(old_content)
        .into_iter()
        .filter(|sig| !new_set.iter().any(|n| n == sig))
        .collect()
}

/// 应用一次修复响应：优先整文件重写，其次精确编辑。
///
/// 任一改写/编辑失败都返回 `Err` —— 闸门不能把「没改成」记成成功。
fn apply_repair_response(raw: &str, repair_context: &str) -> Result<String, String> {
    let value = parse_json_payload(raw)?;

    let rewrites = collect_rewrites(&value);
    if !rewrites.is_empty() {
        // 修复不得删除公开 API：先留快照，应用后比对，发现删除就回滚。
        let mut snapshots: Vec<(PathBuf, Option<String>)> = Vec::new();
        for rw in &rewrites {
            let path = PathBuf::from(&rw.path);
            let before = std::fs::read_to_string(&path).ok();
            snapshots.push((path, before));
        }

        let mut summary = Vec::new();
        for rw in rewrites {
            let content = strip_code_fence(&rw.content);
            sisyphus_tools_system::file::write(Path::new(&rw.path), &content)
                .map_err(|e| format!("rewrite failed for {}: {e}", rw.path))?;
            summary.push(format!("Rewrote {}", rw.path));
        }

        if let Some(violation) = rollback_if_public_api_removed(&snapshots, repair_context) {
            return Err(violation);
        }
        return Ok(summary.join("\n"));
    }

    let edits = parse_edit_requests(raw)?;
    if edits.is_empty() {
        return Err("repair response contained no edits".to_string());
    }
    let mut snapshots: Vec<(PathBuf, Option<String>)> = Vec::new();
    for edit in &edits {
        let path = PathBuf::from(&edit.path);
        let before = std::fs::read_to_string(&path).ok();
        snapshots.push((path, before));
    }

    let mut summary = Vec::new();
    for edit in edits {
        sisyphus_tools_system::file::edit(
            Path::new(&edit.path),
            &edit.old_string,
            &edit.new_string,
        )
        .map_err(|e| format!("edit failed for {}: {e}", edit.path))?;
        summary.push(format!("Applied edit to {}", edit.path));
    }

    if let Some(violation) = rollback_if_public_api_removed(&snapshots, repair_context) {
        return Err(violation);
    }
    Ok(summary.join("\n"))
}

/// 从一条公开 API 声明里取出标识符名（`pub fn add(...)` → `add`）。
fn api_identifier(signature: &str) -> Option<String> {
    let rest = [
        "pub fn ",
        "pub struct ",
        "pub enum ",
        "pub trait ",
        "pub const ",
        "pub static ",
        "pub type ",
        "pub mod ",
        "pub union ",
    ]
    .iter()
    .find_map(|prefix| signature.strip_prefix(prefix))?;
    let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// 若本次改动删除了公开 API，回滚到改动前的内容并返回错误说明。
///
/// 例外：**闸门自己要求删掉**该 API 时允许删除（验证输出里点名了它）。
///
/// 闸门修复的目标是「让命令通过」，不是「重构用户的 API」。实测弱模型会在
/// 全文件重写时顺手删掉不相关的公开函数，而客观闸门（只检查被测行为）照样
/// 通过 —— 破坏就此静默发生。
fn rollback_if_public_api_removed(
    snapshots: &[(PathBuf, Option<String>)],
    verification_output: &str,
) -> Option<String> {
    let mut violations: Vec<String> = Vec::new();
    for (path, before) in snapshots {
        let Some(before) = before else { continue };
        let Ok(after) = std::fs::read_to_string(path) else {
            continue;
        };
        let deleted: Vec<String> = deleted_public_apis(before, &after)
            .into_iter()
            .filter(|sig| {
                // 闸门点名要删的 API → 允许删（否则「删除某个函数」类任务永远做不成）。
                let requested = api_identifier(sig)
                    .map(|name| verification_output.contains(&name))
                    .unwrap_or(false);
                !requested
            })
            .collect();
        if !deleted.is_empty() {
            violations.push(format!("{}: {}", path.display(), deleted.join(", ")));
        }
    }
    if violations.is_empty() {
        return None;
    }
    // 回滚：把文件恢复成改动前的内容。
    for (path, before) in snapshots {
        if let Some(content) = before {
            let _ = sisyphus_tools_system::file::write(path, content);
        }
    }
    Some(format!(
        "repair removed public API (rolled back): {}",
        violations.join("; ")
    ))
}

#[cfg(test)]
#[path = "developer_tests.rs"]
mod tests;
