use super::*;
use sisyphus_tools_llm::metis::{DummyLlmClient, FixedResponseClient};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tempfile::tempdir;

/// T2.1 slice 1：确定性 2-step plan（CreateFile ×2），interrupt/resume 测试用。
const T2_1_SLICE1_PLAN: &str = r#"{
    "goal": "T2.1 Slice1 Goal",
    "overview": "T2.1 Slice1 Overview",
    "steps": [
        {
            "id": 1,
            "description": "Create file A",
            "action": { "type": "CreateFile", "params": { "path": "/tmp/opencode/t2_1_slice1_a.txt", "content": "a" } },
            "expected_outcome": "File exists",
            "verification": { "type": "None" },
            "status": "Pending"
        },
        {
            "id": 2,
            "description": "Create file B",
            "action": { "type": "CreateFile", "params": { "path": "/tmp/opencode/t2_1_slice1_b.txt", "content": "b" } },
            "expected_outcome": "File exists",
            "verification": { "type": "None" },
            "status": "Pending"
        }
    ]
}"#;

#[tokio::test]
async fn test_developer_new() {
    let llm = Arc::new(DummyLlmClient);
    let _dev = Developer::new(llm);
    // Just check it constructs
}

#[tokio::test]
async fn test_developer_with_wisdom() {
    let llm = Arc::new(DummyLlmClient);
    let _dev = Developer::new(llm).with_wisdom();
    // Should not crash; wisdom inject just returns empty if no notepads exist
}

#[tokio::test]
async fn test_plan_parsing() {
    let plan_json = r#"{
        "goal": "Test Goal",
        "overview": "Test Overview",
        "steps": [
            {
                "id": 1,
                "description": "Step 1",
                "action": { "type": "ReadFile", "params": { "path": "test.txt" } },
                "expected_outcome": "File read",
                "verification": { "type": "None" },
                "status": "Pending"
            }
        ]
    }"#;
    let llm = Arc::new(FixedResponseClient(plan_json.to_string()));
    let dev = Developer::new(llm);
    let plan = dev.plan("Test Goal", "Context").await.unwrap();
    assert_eq!(plan.goal, "Test Goal");
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].id, 1);
}

#[tokio::test]
async fn plan_retry_keeps_goal_and_context() {
    // 首次响应用了非法的 verification 变体 → 触发重试。
    // 真实缺陷：旧实现只把报错发给模型，3B 模型因此彻底丢失目标
    // （实测把「拆 add 到 src/math.rs」重试成「新建 Hello World 项目」）。
    let invalid = r#"{"goal":"Test Goal","overview":"x","steps":[{"id":1,"description":"d","action":{"type":"ReadFile","params":{"path":"a"}},"expected_outcome":"e","verification":{"type":"ReadFile","params":{"path":"a"}},"status":"Pending"}]}"#;
    let valid = r#"{"goal":"Test Goal","overview":"x","steps":[{"id":1,"description":"d","action":{"type":"ReadFile","params":{"path":"a"}},"expected_outcome":"e","verification":{"type":"None"},"status":"Pending"}]}"#;
    let client = Arc::new(SequenceClient::new(&[invalid, valid]));
    let dev = Developer::new(client.clone());

    let plan = dev.plan("Test Goal", "PROJECT-CONTEXT-MARKER").await.expect("重试后应解析成功");
    assert_eq!(plan.goal, "Test Goal");

    let prompts = client.prompts.lock().expect("prompts");
    assert_eq!(prompts.len(), 2, "应恰好重试一次");
    assert!(
        prompts[1].contains("Test Goal"),
        "重试 prompt 丢失目标: {}",
        prompts[1]
    );
    assert!(
        prompts[1].contains("PROJECT-CONTEXT-MARKER"),
        "重试 prompt 丢失项目上下文: {}",
        prompts[1]
    );
}

#[tokio::test]
async fn complex_change_edit_failure_is_an_error() {
    // 旧实现把编辑失败收进 "Warning" 却返回 Success，上层会跳过修复。
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing.rs");
    let step = DevStep {
        id: 1,
        description: "complex change".to_string(),
        action: DevAction::ComplexChange {
            description: "把 a 改成 b".to_string(),
        },
        expected_outcome: "edited".to_string(),
        verification: VerificationMethod::None,
        status: StepStatus::Pending,
    };
    let edit_json = format!(
        r#"[{{"path":"{}","old_string":"a","new_string":"b"}}]"#,
        missing.display()
    );
    let dev = Developer::new(Arc::new(SequenceClient::new(&[&edit_json])));

    let err = dev.execute_step(&step).await.expect_err("编辑失败必须报错");
    assert!(
        err.contains("failed to apply"),
        "错误信息应说明失败原因: {err}"
    );
}

#[tokio::test]
async fn test_execute_step_create_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_dev.txt");
    let path_str = path.to_string_lossy().to_string();

    let llm = Arc::new(DummyLlmClient);
    let dev = Developer::new(llm);

    let step = DevStep {
        id: 1,
        description: "Create file".to_string(),
        action: DevAction::CreateFile {
            path: path_str.clone(),
            content: "hello dev".to_string(),
        },
        expected_outcome: "File exists".to_string(),
        verification: VerificationMethod::None,
        status: StepStatus::Pending,
    };

    let result = dev.execute_step(&step).await.unwrap();
    assert_eq!(result.status, StepStatus::Success);
    assert!(path.exists());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "hello dev");
    assert!(
        result.output.contains("Created file"),
        "新建应如实标注: {}",
        result.output
    );
}

#[tokio::test]
async fn create_file_reports_overwrite_of_existing_file() {
    // 覆盖已有文件必须如实标注（实测模型把用户的 Cargo.toml 覆盖成了
    // 另一个项目，静默覆盖让人完全看不出发生了什么）。
    let dir = tempdir().unwrap();
    let path = dir.path().join("Cargo.toml");
    std::fs::write(&path, "[package]\nname = \"real\"\n").unwrap();

    let step = DevStep {
        id: 1,
        description: "Create file".to_string(),
        action: DevAction::CreateFile {
            path: path.to_string_lossy().to_string(),
            content: "[package]\nname = \"hello_world\"\n".to_string(),
        },
        expected_outcome: "File exists".to_string(),
        verification: VerificationMethod::None,
        status: StepStatus::Pending,
    };

    let dev = Developer::new(Arc::new(DummyLlmClient));
    let result = dev.execute_step(&step).await.unwrap();
    assert_eq!(result.status, StepStatus::Success);
    assert!(
        result.output.contains("Overwrote existing file"),
        "覆盖已有文件必须标注: {}",
        result.output
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[package]\nname = \"hello_world\"\n"
    );
}

#[tokio::test]
async fn test_execute_step_read_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_read.txt");
    std::fs::write(&path, "content").unwrap();
    let path_str = path.to_string_lossy().to_string();

    let llm = Arc::new(DummyLlmClient);
    let dev = Developer::new(llm);

    let step = DevStep {
        id: 1,
        description: "Read file".to_string(),
        action: DevAction::ReadFile { path: path_str },
        expected_outcome: "Content read".to_string(),
        verification: VerificationMethod::None,
        status: StepStatus::Pending,
    };

    let result = dev.execute_step(&step).await.unwrap();
    assert_eq!(result.status, StepStatus::Success);
    assert_eq!(result.output, "content");
}

#[tokio::test]
async fn test_execute_step_edit_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_edit.txt");
    std::fs::write(&path, "old content").unwrap();
    let path_str = path.to_string_lossy().to_string();

    let llm = Arc::new(DummyLlmClient);
    let dev = Developer::new(llm);

    let step = DevStep {
        id: 1,
        description: "Edit file".to_string(),
        action: DevAction::EditFile {
            path: path_str,
            old_string: "old content".to_string(),
            new_string: "new content".to_string(),
        },
        expected_outcome: "Content updated".to_string(),
        verification: VerificationMethod::None,
        status: StepStatus::Pending,
    };

    let result = dev.execute_step(&step).await.unwrap();
    assert_eq!(result.status, StepStatus::Success);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "new content");
}

#[tokio::test]
async fn test_verify_step_llm_review() {
    let llm = Arc::new(FixedResponseClient("yes".to_string()));
    let dev = Developer::new(llm);

    let step = DevStep {
        id: 1,
        description: "Test".to_string(),
        action: DevAction::ReadFile {
            path: "any".to_string(),
        },
        expected_outcome: "Any".to_string(),
        verification: VerificationMethod::LlmReview {
            criteria: "is it a string?".to_string(),
        },
        status: StepStatus::Pending,
    };

    let result = StepResult {
        step_id: 1,
        status: StepStatus::Success,
        output: "some output".to_string(),
        verification_output: None,
    };

    let verified = dev.verify_step(&step, &result).await.unwrap();
    assert!(verified);
}

#[tokio::test]
async fn test_format_report() {
    let report = DevelopmentReport {
        goal: "Test Goal".to_string(),
        overall_status: DevOverallStatus::Success,
        steps: vec![StepResult {
            step_id: 1,
            status: StepStatus::Success,
            output: "Done".to_string(),
            verification_output: Some("Verified".to_string()),
        }],
        artifacts: vec!["file.txt".to_string()],
        final_verification: None,
    };

    let formatted = format_report(&report);
    assert!(formatted.contains("Test Goal"));
    assert!(formatted.contains("SUCCESS"));
    assert!(formatted.contains("Step 1"));
    assert!(formatted.contains("file.txt"));
}

#[tokio::test]
async fn test_checkpoint_save_and_load() {
    let dir = tempdir().unwrap();
    let llm = Arc::new(DummyLlmClient);
    let dev = Developer::new(llm).with_checkpoint(dir.path());

    let step_result = StepResult {
        step_id: 1,
        status: StepStatus::Success,
        output: "done".to_string(),
        verification_output: None,
    };

    dev.save_checkpoint("test", "ctx", &[1], std::slice::from_ref(&step_result), &[]);

    let loaded = dev.load_checkpoint().unwrap();
    assert_eq!(loaded.goal, "test");
    assert_eq!(loaded.completed_step_ids, vec![1]);
    assert_eq!(loaded.step_results[0].step_id, 1);
}

#[tokio::test]
async fn test_run_step_with_fix_sequential() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("fix_test.txt");
    let path_str = path.to_string_lossy().to_string();

    // Create a step that creates a file (no fix needed)
    let llm = Arc::new(DummyLlmClient);
    let dev = Developer::new(llm).with_checkpoint(dir.path());

    let step = DevStep {
        id: 1,
        description: "Create file".to_string(),
        action: DevAction::CreateFile {
            path: path_str.clone(),
            content: "hello".to_string(),
        },
        expected_outcome: "File exists".to_string(),
        verification: VerificationMethod::None,
        status: StepStatus::Pending,
    };

    let result = dev.run_step_with_fix(&step).await;
    assert_eq!(result.status, StepStatus::Success);
    assert!(path.exists());
}

#[tokio::test]
async fn test_develop_interrupt_returns_cancelled() {
    let llm = Arc::new(FixedResponseClient(T2_1_SLICE1_PLAN.to_string()));
    let interrupt = Arc::new(AtomicBool::new(true));
    let dev =
        Developer::new(llm).with_checkpoint(tempdir().unwrap().path()).with_interrupt(interrupt);

    let report = dev.develop("T2.1 Slice1 Goal", "ctx", false, false).await.unwrap();
    assert_eq!(report.overall_status, DevOverallStatus::Cancelled);
}

#[tokio::test]
async fn test_develop_resume_skips_completed_steps() {
    let dir = tempdir().unwrap();
    let llm = Arc::new(FixedResponseClient(T2_1_SLICE1_PLAN.to_string()));
    let dev = Developer::new(llm).with_checkpoint(&dir);
    let report = dev.develop("T2.1 Slice1 Goal", "ctx", false, false).await.unwrap();
    assert_eq!(report.overall_status, DevOverallStatus::Success);
    assert_eq!(report.steps.len(), 2);

    let dev2 = Developer::new(Arc::new(FixedResponseClient(T2_1_SLICE1_PLAN.to_string())))
        .with_checkpoint(&dir);
    let resumed = dev2.develop("T2.1 Slice1 Goal", "ctx", false, true).await.unwrap();
    assert_eq!(resumed.overall_status, DevOverallStatus::Success);
    assert_eq!(resumed.steps.len(), 2);
}

struct DenyApprover;

#[async_trait::async_trait]
impl DevApprover for DenyApprover {
    async fn approve(&self, _tool_id: &str, _action: &serde_json::Value) -> Result<bool, String> {
        Ok(false)
    }
}

struct AllowApprover;

#[async_trait::async_trait]
impl DevApprover for AllowApprover {
    async fn approve(&self, _tool_id: &str, _action: &serde_json::Value) -> Result<bool, String> {
        Ok(true)
    }
}

#[tokio::test]
async fn test_develop_approval_denies_step() {
    let llm = Arc::new(FixedResponseClient(T2_1_SLICE1_PLAN.to_string()));
    let dev = Developer::new(llm).with_approval_handler(Arc::new(DenyApprover));
    let report = dev.develop("T2.1 Slice1 Goal", "ctx", false, false).await.unwrap();
    assert_eq!(report.steps.len(), 2);
    assert!(report.steps.iter().all(|r| !matches!(r.status, StepStatus::Success)));
    assert!(format!("{:?}", report.steps[0].status).contains("denied by user"));
}

#[test]
fn complex_change_maps_to_file_edit_tool_id() {
    // ComplexChange 之前返回 None → 整个审批闸被跳过（deny 规则拦不住）。
    let action = DevAction::ComplexChange {
        description: "批量修改".to_string(),
    };
    assert_eq!(
        dev_tool_id(&action),
        Some("builtin:file_edit"),
        "批量文件编辑必须走同一套权限规则"
    );
}

#[tokio::test]
async fn test_develop_approval_allows_step() {
    let llm = Arc::new(FixedResponseClient(T2_1_SLICE1_PLAN.to_string()));
    let dev = Developer::new(llm).with_approval_handler(Arc::new(AllowApprover));
    let report = dev.develop("T2.1 Slice1 Goal", "ctx", false, false).await.unwrap();
    assert_eq!(report.overall_status, DevOverallStatus::Success);
    assert_eq!(report.steps.len(), 2);
    assert!(report.steps.iter().all(|r| matches!(r.status, StepStatus::Success)));
}

// ── 修复环节的 JSON 解析健壮性（弱模型兼容）────────────────────

#[test]
fn parse_edit_requests_accepts_bare_array() {
    let raw = r#"[{"path":"a.rs","old_string":"x","new_string":"y"}]"#;
    let edits = parse_edit_requests(raw).expect("裸数组应可解析");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].path, "a.rs");
}

#[test]
fn parse_edit_requests_accepts_single_object() {
    // 3B 模型常见：把数组元素当整体返回
    let raw = r#"{"path":"a.rs","old_string":"x","new_string":"y"}"#;
    let edits = parse_edit_requests(raw).expect("单对象应可解析");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_string, "y");
}

#[test]
fn parse_edit_requests_accepts_wrapped_array_and_fence() {
    let raw =
        "```json\n{\"edits\":[{\"path\":\"a.rs\",\"old_string\":\"x\",\"new_string\":\"y\"}]}\n```";
    let edits = parse_edit_requests(raw).expect("包裹层 + 围栏应可解析");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].path, "a.rs");
}

#[test]
fn parse_edit_requests_rejects_non_edit_payload() {
    assert!(parse_edit_requests("\"plain string\"").is_err());
    assert!(parse_edit_requests(r#"{"foo":1}"#).is_err());
}

// ── 客观验收门（dev --verify-cmd）──────────────────────────────

/// 按顺序返回多个响应的测试客户端：闸门场景需要「计划」与「修复」两份不同回复。
struct SequenceClient {
    responses: std::sync::Mutex<std::collections::VecDeque<String>>,
    prompts: std::sync::Mutex<Vec<String>>,
}

impl SequenceClient {
    fn new(responses: &[&str]) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses.iter().map(|s| s.to_string()).collect()),
            prompts: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl sisyphus_tools_llm::metis::LlmClient for SequenceClient {
    async fn chat(
        &self,
        _system: &str,
        user: &str,
        _json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        if let Ok(mut prompts) = self.prompts.lock() {
            prompts.push(user.to_string());
        }
        let next = self
            .responses
            .lock()
            .map(|mut q| q.pop_front().unwrap_or_default())
            .unwrap_or_default();
        Ok(next)
    }

    async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        self.chat("", prompt, false).await
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        ("test".into(), "http://test".into(), "test-model".into())
    }

    fn breaker(&self) -> sisyphus_types::CircuitBreaker {
        sisyphus_types::CircuitBreaker::default()
    }
}

/// 单步计划：在工作目录写一个文件（验证方式由模型自选为 None）。
fn single_create_plan(path: &str) -> String {
    format!(
        r#"{{"goal":"gate","overview":"one step","steps":[{{"id":1,"description":"create","action":{{"type":"CreateFile","params":{{"path":"{path}","content":"x"}}}},"expected_outcome":"file exists","verification":{{"type":"None"}},"status":"Pending"}}]}}"#
    )
}

#[tokio::test]
async fn final_verification_passes_when_command_succeeds() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("a.txt");
    let plan = single_create_plan(&target.to_string_lossy());

    let dev = Developer::new(Arc::new(FixedResponseClient(plan)))
        .with_final_verification("true")
        .with_final_fix_attempts(2);
    let report = dev.develop("gate goal", "", false, false).await.unwrap();

    assert_eq!(report.overall_status, DevOverallStatus::Success);
    let gate = report.final_verification.expect("应记录闸门结果");
    assert!(gate.passed, "`true` 应判定通过");
    assert!(gate.initial_passed, "一次就过时首轮应为通过");
    assert_eq!(gate.fix_attempts, 0);
}

#[tokio::test]
async fn final_verification_overrides_model_declared_success() {
    // 这是 S2 暴露的真实缺陷：所有步骤自报 Success（模型自选的 verification 通过），
    // 但用户目标命令失败 → 整体必须判 Failed。
    let dir = tempdir().unwrap();
    let target = dir.path().join("b.txt");
    let plan = single_create_plan(&target.to_string_lossy());

    let dev = Developer::new(Arc::new(FixedResponseClient(plan)))
        .with_final_verification("false")
        .with_final_fix_attempts(0);
    let report = dev.develop("gate goal", "", false, false).await.unwrap();

    assert!(target.exists(), "步骤本身确实执行成功了");
    assert!(
        matches!(report.overall_status, DevOverallStatus::Failed(_)),
        "步骤成功不能覆盖客观验收失败，实际: {:?}",
        report.overall_status
    );
    let gate = report.final_verification.expect("应记录闸门结果");
    assert!(!gate.passed);
    assert_eq!(gate.fix_attempts, 0);
}

#[tokio::test]
async fn final_verification_fix_attempt_closes_the_loop() {
    // 约定命令检查 marker 内容；首次失败 → LLM 返回编辑 → 第二次通过。
    let dir = tempdir().unwrap();
    let marker = dir.path().join("marker.txt");
    std::fs::write(&marker, "bad").unwrap();
    let target = dir.path().join("c.txt");
    let plan = single_create_plan(&target.to_string_lossy());
    let fix = format!(
        r#"[{{"path":"{}","old_string":"bad","new_string":"good"}}]"#,
        marker.to_string_lossy()
    );
    // 失败时把「路径:行号:内容」打到 stderr/stdout —— 真实 clippy/rustc 也这样，
    // 闸门的修复环节据此读回文件内容。
    let verify = format!(
        "grep -q good {m} || {{ grep -Hn \"\" {m}; exit 1; }}",
        m = marker.display()
    );

    let llm = Arc::new(SequenceClient::new(&[&plan, &fix]));
    let dev =
        Developer::new(llm.clone()).with_final_verification(verify).with_final_fix_attempts(2);
    let report = dev.develop("gate goal", "", false, false).await.unwrap();

    let gate = report.final_verification.expect("应记录闸门结果");
    assert!(gate.passed, "修复后闸门应通过，输出: {}", gate.output);
    assert!(!gate.initial_passed, "首轮失败应被记录，供修复率统计");
    assert_eq!(gate.fix_attempts, 1);
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "good");
    // 修复步骤进入报告，便于审计
    assert!(
        report.steps.iter().any(|s| s.step_id == 9001),
        "应记录闸门修复步骤，实际: {:?}",
        report.steps.iter().map(|s| s.step_id).collect::<Vec<_>>()
    );
    // 修复 prompt 必须包含被点名的文件内容，否则弱模型无法匹配 old_string
    let prompts = llm.prompts.lock().unwrap();
    let fix_prompt = prompts.iter().find(|p| p.contains("objective verification command failed"));
    let fix_prompt = fix_prompt.expect("应有修复 prompt");
    assert!(
        fix_prompt.contains("marker.txt"),
        "修复 prompt 应包含文件路径"
    );
    assert!(fix_prompt.contains("bad"), "修复 prompt 应包含当前文件内容");
}

#[test]
fn truncate_chars_is_multibyte_safe() {
    assert_eq!(truncate_chars("abc", 5), "abc");
    let cut = truncate_chars("中文中文", 2);
    assert!(cut.starts_with("中文"));
    assert!(cut.ends_with("[truncated]"));
}

// ── 闸门修复的两种响应形态 ────────────────────────────────────

#[test]
fn apply_repair_response_rewrites_whole_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.rs");
    std::fs::write(&file, "broken").unwrap();

    let raw = format!(
        r#"{{"files":[{{"path":"{}","content":"pub fn ok() {{}}\n"}}]}}"#,
        file.display()
    );
    let summary = apply_repair_response(&raw).expect("整文件重写应成功");
    assert!(summary.contains("Rewrote"), "summary: {summary}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "pub fn ok() {}\n");
}

#[test]
fn apply_repair_response_accepts_bare_rewrite_object_and_fence() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("b.rs");
    std::fs::write(&file, "broken").unwrap();

    let raw = format!(
        "```json\n{{\"path\":\"{}\",\"content\":\"fixed\"}}\n```",
        file.display()
    );
    apply_repair_response(&raw).expect("单对象 + 围栏应成功");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "fixed");
}

#[test]
fn apply_repair_response_falls_back_to_edits() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("c.rs");
    std::fs::write(&file, "let x = 1;").unwrap();

    let raw = format!(
        r#"[{{"path":"{}","old_string":"let x = 1;","new_string":"let x = 2;"}}]"#,
        file.display()
    );
    apply_repair_response(&raw).expect("编辑形态应成功");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "let x = 2;");
}

#[test]
fn apply_repair_response_errors_on_stale_old_string() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("d.rs");
    std::fs::write(&file, "actual").unwrap();

    let raw = format!(
        r#"[{{"path":"{}","old_string":"imagined","new_string":"x"}}]"#,
        file.display()
    );
    let err = apply_repair_response(&raw).expect_err("不匹配必须报错，不能记成成功");
    assert!(err.contains("edit failed"), "err: {err}");
    // 文件未被改动
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "actual");
}

#[test]
fn apply_repair_response_rejects_empty_payload() {
    assert!(apply_repair_response(r#"{"files":[]}"#).is_err());
    assert!(apply_repair_response("not json").is_err());
}

#[test]
fn strip_code_fence_removes_outer_markdown_fence() {
    // S2 实测：内容正确但套了 ```rust 围栏，落盘后编译失败
    assert_eq!(
        strip_code_fence("```rust\npub fn f() {}\n```"),
        "pub fn f() {}\n"
    );
    assert_eq!(strip_code_fence("```\nplain\n```"), "plain\n");
    // 没有围栏（或只有一半）时原样保留
    assert_eq!(strip_code_fence("pub fn f() {}"), "pub fn f() {}");
    assert_eq!(strip_code_fence("```rust\nunclosed"), "```rust\nunclosed");
}

#[test]
fn apply_repair_response_strips_fence_in_rewrite_content() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("e.rs");
    std::fs::write(&file, "broken").unwrap();

    let raw = format!(
        r#"{{"files":[{{"path":"{}","content":"```rust\npub fn ok() {{}}\n```"}}]}}"#,
        file.display()
    );
    apply_repair_response(&raw).expect("应成功");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "pub fn ok() {}\n");
}
