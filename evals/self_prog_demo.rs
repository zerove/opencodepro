//! 闭环自我编程 (Self-Programming Loop) 端到端评测（P2）
//!
//! 与 `golden-sop-exec` 等离线评测不同，本评测器**真实地**驱动
//! `SelfProgrammingLoop::close_the_loop`：
//! 1. 构造一个固定的 `ToolGap`（模拟 GapAnalyzer 产出）
//! 2. 用 `FixedResponseClient` 提供 LLM 响应（无需外部 API key）
//! 3. 用一个 mock `Verifier`（前 N-1 次失败，最后一次通过）
//! 4. 用 `FileRegistrar` 落盘生成的 Rust 工具
//! 5. 把每次尝试的 AttemptRecord 追加到 JSONL 历史
//!
//! 输出：
//! - 终端：成功/失败、最终 tool_id、尝试次数、落盘路径
//! - `.sisyphus/eval-history/self-prog-{ts}.jsonl`
//!   字段：timestamp、tool_id、attempts、success、final_code、output_path

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sisyphus_intel::gap_analyzer::{GapAnalyzer, ToolGap};
use sisyphus_intel::self_programming::{
    AttemptRecord, FileRegistrar, LoopConfig, SelfProgrammingLoop, ToolGenerationResult, Verifier,
    VerificationOutcome,
};
use sisyphus_intel::tool_coder::ToolCoder;
use sisyphus_tools_llm::{FixedResponseClient, LlmClient};
use sisyphus_types::IntelError;
use sisyphus_types::tools::ToolSpec;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize)]
struct HistoryEntry {
    timestamp: u64,
    tool_id: String,
    success: bool,
    attempts: usize,
    final_code: Option<String>,
    output_path: Option<String>,
    diagnostics: Vec<String>,
}

/// 一个共享的、可配置的 mock 验证器：
/// - 第一次 verify 返回 failed（带诊断）
/// - 第二次 verify 返回 passed
/// - 后续永远 passed
struct EventuallyPassingVerifier {
    failed_count_target: usize,
}

#[async_trait]
impl Verifier for EventuallyPassingVerifier {
    async fn verify(&self, _code: &str) -> Result<VerificationOutcome, IntelError> {
        // 每次调用都用 SystemTime 派生确定性结果；为简化用 atomic 计数
        // （更简单：使用 thread_local 计数器）
        thread_local! {
            static COUNTER: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        }
        let call_idx = COUNTER.with(|c| {
            let v = c.get();
            c.set(v + 1);
            v
        });
        if (call_idx as usize) < self.failed_count_target {
            Ok(VerificationOutcome {
                passed: false,
                diagnostics: format!(
                    "Mock verifier: call #{} fails (target {} fails before pass)",
                    call_idx + 1,
                    self.failed_count_target
                ),
            })
        } else {
            Ok(VerificationOutcome {
                passed: true,
                diagnostics: "Mock verifier: passed".to_string(),
            })
        }
    }
}

fn make_tool_gap() -> ToolGap {
    ToolGap {
        gap_id: format!("gap-{}", now_secs()),
        required_capability: "compute_fibonacci".to_string(),
        failing_node: "node_compute_fib".to_string(),
        suggested_tool_spec: ToolSpec {
            tool_id: format!("tool-fib-{}", now_secs()),
            required_capability: "compute_fibonacci".to_string(),
            expected_input: serde_json::json!({"n": "u64"}),
            expected_output: serde_json::json!({"value": "u64"}),
            success_criteria: "returns the n-th Fibonacci number for n in [0, 92]".to_string(),
            test_command: Some("cargo test".to_string()),
            workdir: None,
        },
    }
}

/// LLM 返回一段可被 `extract_code` 接受的 Rust 代码：
/// ```rust
/// pub fn handle(n: u64) -> u64 {
///     let (mut a, mut b) = (0u64, 1u64);
///     for _ in 0..n { let c = a + b; a = b; b = c; }
///     a
/// }
/// ```
fn llm_response() -> String {
    let body = r#"pub fn handle(n: u64) -> u64 {
    let (mut a, mut b) = (0u64, 1u64);
    for _ in 0..n {
        let c = a + b;
        a = b;
        b = c;
    }
    a
}"#;
    format!("```rust\n{body}\n```")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ensure_history_dir() -> PathBuf {
    let dir = PathBuf::from(".sisyphus/eval-history");
    fs::create_dir_all(&dir).expect("create eval-history dir");
    dir
}

#[tokio::main]
async fn main() {
    // 1) LLM 客户端：返回固定代码
    let llm: Arc<dyn LlmClient> = Arc::new(FixedResponseClient(llm_response()));

    // 2) 构造 loop 所需的全部依赖
    let gap_analyzer = Arc::new(GapAnalyzer::new(llm.clone()));
    let coder = Arc::new(ToolCoder::new(llm.clone()));
    // 第一次 verify 失败，第二次通过；保证 max_attempts >= 2
    let verifier: Arc<dyn Verifier> = Arc::new(EventuallyPassingVerifier {
        failed_count_target: 1,
    });

    // 3) FileRegistrar 写入 .sisyphus/eval-history/self_prog_tools/
    let tool_dir = ensure_history_dir().join("self_prog_tools");
    let registrar: Arc<dyn sisyphus_intel::self_programming::ToolRegistrar> =
        Arc::new(FileRegistrar::new(tool_dir));

    let config = LoopConfig {
        max_attempts: 3,
        enabled: true,
    };

    let sp_loop = SelfProgrammingLoop::new(gap_analyzer, coder, verifier, registrar, config);

    // 4) 准备 gap
    let gap = make_tool_gap();
    println!("▶ 启动 SelfProgrammingLoop，目标工具: {}", gap.suggested_tool_spec.tool_id);
    println!("  需求: {}", gap.required_capability);

    // 5) 执行闭环
    let result: ToolGenerationResult = match sp_loop.close_the_loop(&gap).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ 闭环失败: {e}");
            std::process::exit(1);
        }
    };

    // 6) 渲染结果
    let attempts = &result.attempts;
    println!();
    println!("── 闭环结果 ──");
    println!("  tool_id:    {}", result.tool_id);
    println!("  success:    {}", result.success);
    println!("  attempts:   {}", attempts.len());
    for (i, a) in attempts.iter().enumerate() {
        println!(
            "    [{}] passed={} diagnostics={}",
            i + 1,
            a.passed,
            a.diagnostics
        );
    }
    if let Some(p) = &result.output_path {
        println!("  落盘路径:   {}", p.display());
    } else {
        println!("  落盘路径:   (未生成)");
    }
    if let Some(code) = &result.final_code {
        println!("  最终代码 ({} chars):", code.len());
        for line in code.lines().take(8) {
            println!("    {line}");
        }
        if code.lines().count() > 8 {
            println!("    ... ({} more lines)", code.lines().count() - 8);
        }
    }

    // 7) 落 JSONL 历史
    let dir = ensure_history_dir();
    let ts = now_secs();
    let path = dir.join(format!("self-prog-{ts}.jsonl"));
    let entry = HistoryEntry {
        timestamp: ts,
        tool_id: result.tool_id.clone(),
        success: result.success,
        attempts: attempts.len(),
        final_code: result.final_code.clone(),
        output_path: result.output_path.as_ref().map(|p| p.display().to_string()),
        diagnostics: attempts.iter().map(|a: &AttemptRecord| a.diagnostics.clone()).collect(),
    };
    let line = serde_json::to_string(&entry).expect("serialize history entry");
    fs::write(&path, format!("{line}\n")).expect("write history");
    println!();
    println!("✓ 历史已写入: {}", path.display());

    // 8) 退出码
    if result.success {
        std::process::exit(0);
    } else {
        std::process::exit(2);
    }
}
