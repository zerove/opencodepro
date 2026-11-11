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
