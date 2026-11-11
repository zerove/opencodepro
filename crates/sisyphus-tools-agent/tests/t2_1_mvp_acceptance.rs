//! T2.1「TUI 对话式执行 MVP」验收证据（脚本化 mock，不依赖真实 LLM / 凭据）。
//!
//! 覆盖主计划验收条目：一次会话含 ≥2 次工具调用、1 次审批、1 次中断恢复。
//!
//! 场景：2 步确定性计划（CreateFile ×2）
//! 1. 第一段运行：第 1 步审批通过后置位中断标志 → 第 2 步开始前命中中断 → 返回 `Cancelled`，
//!    checkpoint 保留第 1 步结果；
//! 2. 第二段运行（resume=true）：跳过已完成第 1 步，执行第 2 步 → `Success`。

use async_trait::async_trait;
use sisyphus_tools_agent::developer::{DevApprover, DevOverallStatus, Developer};
use sisyphus_tools_llm::metis::FixedResponseClient;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// 2 步计划模板（`%%A%%` / `%%B%%` 为产物路径占位）。
const PLAN_TEMPLATE: &str = r#"{
    "goal": "T2.1 MVP acceptance",
    "overview": "Two-step deterministic plan",
    "steps": [
        {
            "id": 1,
            "description": "Create file A",
            "action": { "type": "CreateFile", "params": { "path": "%%A%%", "content": "a" } },
            "expected_outcome": "File A exists",
            "verification": { "type": "None" },
            "status": "Pending"
        },
        {
            "id": 2,
            "description": "Create file B",
            "action": { "type": "CreateFile", "params": { "path": "%%B%%", "content": "b" } },
            "expected_outcome": "File B exists",
            "verification": { "type": "None" },
            "status": "Pending"
        }
    ]
}"#;

/// 记录审批次数的 approver：首次审批后置位中断标志（模拟用户审批后中断）。
struct RecordingApprover {
    asks: Arc<AtomicUsize>,
    interrupt: Arc<AtomicBool>,
}

#[async_trait]
impl DevApprover for RecordingApprover {
    async fn approve(&self, _tool_id: &str, _action: &serde_json::Value) -> Result<bool, String> {
        self.asks.fetch_add(1, Ordering::SeqCst);
        if self.asks.load(Ordering::SeqCst) == 1 {
            // 第一次审批后用户中断：第 2 步开始前命中中断检查。
            self.interrupt.store(true, Ordering::SeqCst);
        }
        Ok(true)
    }
}

#[tokio::test]
async fn t2_1_mvp_two_tool_calls_one_approval_one_interrupt_and_resume() {
    let dir = tempfile::tempdir().unwrap();
    let file_a = dir.path().join("step_a.txt");
    let file_b = dir.path().join("step_b.txt");
    let plan = PLAN_TEMPLATE
        .replace("%%A%%", &file_a.to_string_lossy())
        .replace("%%B%%", &file_b.to_string_lossy());

    let asks = Arc::new(AtomicUsize::new(0));

    // ── 第 1 段：执行第 1 步后被中断 ──
    let interrupt = Arc::new(AtomicBool::new(false));
    let dev = Developer::new(Arc::new(FixedResponseClient(plan.clone())))
        .with_checkpoint(dir.path())
        .with_interrupt(interrupt.clone())
        .with_approval_handler(Arc::new(RecordingApprover {
            asks: asks.clone(),
            interrupt: interrupt.clone(),
        }));

    let first = dev
        .develop("T2.1 MVP acceptance", "ctx", false, false)
        .await
        .expect("first run should return a report");

    assert_eq!(
        first.overall_status,
        DevOverallStatus::Cancelled,
        "第 1 段应在中断点返回 Cancelled"
    );
    assert_eq!(first.steps.len(), 1, "中断前只完成第 1 步");
    assert!(file_a.exists(), "第 1 步产物应已落盘");
    assert!(!file_b.exists(), "第 2 步不应执行");

    // checkpoint 保留已完成步骤，供恢复使用
    let checkpoint = dev.load_checkpoint().expect("checkpoint should be saved for resume");
    assert_eq!(checkpoint.completed_step_ids, vec![1]);

    // ── 第 2 段：恢复执行剩余步骤 ──
    let resume_interrupt = Arc::new(AtomicBool::new(false));
    let dev2 = Developer::new(Arc::new(FixedResponseClient(plan.clone())))
        .with_checkpoint(dir.path())
        .with_interrupt(resume_interrupt)
        .with_approval_handler(Arc::new(RecordingApprover {
            asks: asks.clone(),
            interrupt: Arc::new(AtomicBool::new(false)),
        }));

    let resumed = dev2
        .develop("T2.1 MVP acceptance", "ctx", false, true)
        .await
        .expect("resume run should return a report");

    assert_eq!(
        resumed.overall_status,
        DevOverallStatus::Success,
        "恢复后应完成剩余步骤"
    );
    assert_eq!(resumed.steps.len(), 2, "恢复后累计 2 步结果");
    assert!(file_a.exists() && file_b.exists(), "两处产物均应存在");

    // 验收条目计数
    let approval_count = asks.load(Ordering::SeqCst);
    assert_eq!(approval_count, 2, "两次受控工具调用各触发一次审批（≥1）");
    assert!(approval_count >= 1, "验收要求至少 1 次审批");
}
