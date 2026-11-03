//! 闭环自编程 (Self-Programming) 编译级端到端评测
//!
//! 旧版本驱动的是已删除的 legacy `SelfProgrammingLoop`（其 "Verifier" 只会
//! 返回布尔值，`FileRegistrar` 只落盘 `.rs` 源码——全程不碰 rustc，等于
//! 给一个从未编译过的字符串发合格证）。
//!
//! 现在的评测驱动**当前生产使用的真实组件链**（全部在 `sisyphus-intel`，
//! 不需要 wasmtime 依赖，因此保持 evals 轻量）：
//!
//! 1. [`ToolCoder::generate_code`] — LLM 产出 Rust 源码（离线用固定响应）
//! 2. [`WasmCompiler::compile`] — **真实 rustc** 编译 `wasm32-unknown-unknown`
//!    cdylib，产出满足沙箱 ABI（`execute` 导出）的 `.wasm`
//! 3. [`ToolVerifier::verify`] — 静态 WASM 契约校验（导出表）+ LLM 语义审计
//! 4. 重试循环 — 编译/校验失败时把 rustc stderr 与失败原因回灌 coder
//!
//! 输出：
//! - 终端：成功/失败、最终 tool_id、尝试次数、落盘的 wasm 路径与大小
//! - `.sisyphus/eval-history/self-prog-{ts}.jsonl`
//!   字段：timestamp、tool_id、attempts、success、wasm_path、wasm_bytes、
//!   exit 码 0/成功 2/失败

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sisyphus_intel::tool_coder::{ToolCoder, REQUIRED_ENTRY_POINT};
use sisyphus_intel::tool_verifier::ToolVerifier;
use sisyphus_intel::wasm_compiler::WasmCompiler;
use sisyphus_tools_llm::LlmClient;
use sisyphus_types::tools::ToolSpec;
use sisyphus_types::TypesError;
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
    wasm_path: Option<String>,
    wasm_bytes: Option<u64>,
    last_failures: Vec<String>,
}

/// 一个按角色路由的离线 LLM：与生产接线一致地**共享同一个客户端**，
/// 但对两类提示词给出各自确定性的答复——
/// - ToolCoder 的生成请求（system 里是 "Senior Rust Engineer"）→ 返回一个
///   满足 `{REQUIRED_ENTRY_POINT}` 的完整源文件 code block
/// - ToolVerifier 的语义审计（system 里是 "Tool Auditor"）→ 返回裸分数
struct OfflineClient {
    code: String,
    audit_score: String,
}

#[async_trait]
impl LlmClient for OfflineClient {
    async fn chat(
        &self,
        system: &str,
        _user: &str,
        _json_mode: bool,
    ) -> Result<String, TypesError> {
        if system.contains("Tool Auditor") {
            Ok(self.audit_score.clone())
        } else {
            Ok(self.code.clone())
        }
    }

    async fn generate(&self, _prompt: &str) -> Result<String, TypesError> {
        Ok(self.code.clone())
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "demo-model".into(),
            "http://demo.invalid".into(),
            "demo-key".into(),
        )
    }

    fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
        sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
    }
}

/// 生成代码：解析输入 JSON 里的 `{"n": <u64>}`，返回 n 阶斐波那契。
///
/// 只有 std，无 serde——与 ToolCoder 给生成器的硬约束一致
/// （bare `rustc` 编译，外层 ABI 适配层由编译器自动追加）。
const GENERATED_CODE: &str = r#"pub fn run(input: &str) -> Result<String, String> {
    let n = match json_u64_field(input, "n") {
        Some(value) => value,
        None => return Err(format!("missing numeric field `n` in input: {input}")),
    };
    if n > 92 {
        return Err(format!("n={n} overflows u64 fibonacci (max 92)"));
    }
    let (mut a, mut b) = (0u64, 1u64);
    for _ in 0..n {
        let next = match a.checked_add(b) {
            Some(v) => v,
            None => return Err(format!("fibonacci overflow at n={n}")),
        };
        a = b;
        b = next;
    }
    Ok(format!("{{\"value\":{a}}}"))
}

/// 在 JSON 文本里找 `"key"` 对应的无符号整数字段（手写解析，避开 serde）。
fn json_u64_field(input: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let key_pos = input.find(&needle)?;
    let after_key = &input[key_pos + needle.len()..];
    let digits_start = after_key.find(':')? + 1;
    let rest = after_key[digits_start..].trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok()
}"#;

fn llm_code_response() -> String {
    format!("```rust\n{GENERATED_CODE}\n```")
}

fn make_tool_spec(ts: u64) -> ToolSpec {
    let tool_id = format!("demo/fib_{ts}");
    ToolSpec {
        tool_id: tool_id.clone(),
        required_capability: "compute_fibonacci".to_string(),
        expected_input: serde_json::json!({"n": 5}),
        expected_output: serde_json::json!({"value": 5}),
        success_criteria: "returns the n-th Fibonacci number for n in [0, 92]".to_string(),
        test_command: None,
        workdir: None,
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn ensure_history_dir() -> PathBuf {
    let dir = PathBuf::from(".sisyphus/eval-history");
    fs::create_dir_all(&dir).expect("create eval-history dir");
    dir
}

#[tokio::main]
async fn main() {
    // 1) 共享 LLM 客户端（与生产接线一致）：按角色路由固定响应
    let llm: Arc<dyn LlmClient> = Arc::new(OfflineClient {
        code: llm_code_response(),
        audit_score: "0.92".into(),
    });

    // 2) 真实工具链组件
    let tools_dir = ensure_history_dir().join("self_prog_tools");
    fs::create_dir_all(&tools_dir).expect("create self-prog tools dir");
    let coder = ToolCoder::new(llm.clone());
    let verifier = ToolVerifier::new(llm);
    let compiler = WasmCompiler::with_tools_dir(&tools_dir);

    let spec = make_tool_spec(now_secs());
    println!("▶ 启动自编程评测，目标工具: {}", spec.tool_id);
    println!("  入口契约: {REQUIRED_ENTRY_POINT}");

    // 3) 复刻 SelfProgrammingManager::resolve_gap 的重试结构
    //    （生成 → 真实编译 → 静态+语义校验 → 失败反馈回灌）
    let max_attempts = 3usize;
    let mut feedback: Option<String> = None;
    let mut wasm_path: Option<PathBuf> = None;
    let mut wasm_bytes: Option<u64> = None;
    let mut last_failures: Vec<String> = Vec::new();
    let mut attempts_used = 0usize;

    for attempt in 1..=max_attempts {
        attempts_used = attempt;
        let code = match coder.generate_code(&spec, feedback.clone()).await {
            Ok(code) => code,
            Err(e) => {
                eprintln!("✗ 代码生成失败: {e}");
                std::process::exit(1);
            },
        };

        // 真实 rustc → wasm32-unknown-unknown
        match compiler.compile(&spec.tool_id, &code).await {
            Ok(path) => {
                println!("  [{attempt}/3] rustc 编译成功 → {}", path.display());

                // 静态 WASM 契约校验 + 语义审计（两个都必须通过）
                let report = match verifier.verify(&spec, &code, &path).await {
                    Ok(report) => report,
                    Err(e) => {
                        eprintln!("✗ 校验器调用失败: {e}");
                        std::process::exit(1);
                    },
                };
                if report.is_deployable() {
                    wasm_path = Some(path.clone());
                    wasm_bytes = fs::metadata(&path).ok().map(|m| m.len());
                    let score = report.confidence_score;
                    println!("  [{attempt}/3] 验证通过 (confidence={score})");
                    last_failures.clear();
                    break;
                }
                println!("  [{attempt}/3] 校验未通过: {:?}", report.failures);
                last_failures = report.failures.clone();
                feedback = Some(format!(
                    "Attempt {attempt} verification failures:\n{}",
                    last_failures.join("\n")
                ));
            },
            Err(e) => {
                // rustc 的 stderr 摘要会通过 Display 带出来，回灌下一轮
                println!("  [{attempt}/3] 编译失败: {e}");
                last_failures = vec![e.to_string()];
                feedback = Some(format!("Attempt {attempt} failed to compile:\n{e}"));
            },
        }
        if attempt == max_attempts {
            break;
        }
    }

    let success = wasm_path.is_some();
    println!();
    println!("── 闭环结果 ──");
    println!("  tool_id:    {}", spec.tool_id);
    println!("  success:    {success}");
    println!("  attempts:   {attempts_used}");
    if let Some(p) = &wasm_path {
        println!("  产物路径:   {}", p.display());
        if let Some(bytes) = wasm_bytes {
            println!("  产物大小:   {bytes} bytes");
        }
    } else {
        println!("  产物路径:   (未生成)");
        for failure in &last_failures {
            println!("  最后失败:   {failure}");
        }
    }

    // 4) 落 JSONL 历史
    let dir = ensure_history_dir();
    let ts = now_secs();
    let path = dir.join(format!("self-prog-{ts}.jsonl"));
    let entry = HistoryEntry {
        timestamp: ts,
        tool_id: spec.tool_id.clone(),
        success,
        attempts: attempts_used,
        wasm_path: wasm_path.as_ref().map(|p| p.display().to_string()),
        wasm_bytes,
        last_failures,
    };
    let line = serde_json::to_string(&entry).expect("serialize history entry");
    fs::write(&path, format!("{line}\n")).expect("write history");
    println!();
    println!("✓ 历史已写入: {}", path.display());

    if success {
        std::process::exit(0);
    } else {
        std::process::exit(2);
    }
}
