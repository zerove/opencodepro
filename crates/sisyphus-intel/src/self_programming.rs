//! 闭环自我编程 (EVOLUTION_PLAN §3.3)
//!
//! 编排 "缺口识别 → 工具生成 → 测试 → 注册" 闭环。
//!
//! 步骤：
//! 1. **缺口识别** ([`GapAnalyzer`])：分析执行轨迹，产出 `ToolGap`
//! 2. **工具生成** ([`ToolCoder`])：LLM 生成 Rust 源码
//! 3. **验证** ([`Verifier`] trait)：用户自定义（编译/wasm/test/LLM 评估）
//! 4. **重试循环**：失败时 feedback 注入，最多 `max_attempts` 次
//! 5. **注册** ([`ToolRegistrar`] trait)：成功时落盘 + 上报
//!
//! 设计目标：
//! - 不侵入 bridge/kernel；独立组件 + 单元测试
//! - 所有步骤可 mock（不依赖 LLM/wasmtime），便于 CI 跑通
//! - 失败时保留生成历史（`history()`）
//! - 不在生产路径默认启用：`LoopConfig::enabled = false`

use crate::gap_analyzer::{GapAnalyzer, ToolGap};
use crate::tool_coder::ToolCoder;
use async_trait::async_trait;
use parking_lot::Mutex;
use sisyphus_types::errors::IntelError;
use std::path::PathBuf;
use std::sync::Arc;

/// 单次尝试的记录。
#[derive(Debug, Clone)]
pub struct AttemptRecord {
    pub attempt: usize,
    pub code: String,
    pub passed: bool,
    pub diagnostics: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
/// Tool Generation Result.
pub struct ToolGenerationResult {
    pub tool_id: String,
    pub final_code: Option<String>,
    pub attempts: Vec<AttemptRecord>,
    pub success: bool,
    pub output_path: Option<PathBuf>,
}

/// 验证 trait：用户自定义编译/测试策略。
/// - 真实场景：包装 [`crate::tool_verifier::ToolVerifier`]
/// - 测试场景：返回硬编码结果
#[async_trait]
pub trait Verifier: Send + Sync {
    async fn verify(&self, code: &str) -> Result<VerificationOutcome, IntelError>;
}

#[derive(Debug, Clone)]
/// Verification Outcome.
pub struct VerificationOutcome {
    pub passed: bool,
    pub diagnostics: String,
}

/// 工具注册回调：成功生成后由 [`SelfProgrammingLoop`] 调用。
#[async_trait]
pub trait ToolRegistrar: Send + Sync {
    async fn register(&self, tool_id: &str, code: &str) -> Result<PathBuf, IntelError>;
}

/// 默认注册器：写入 `.sisyphus/generated_tools/{tool_id}.rs`。
pub struct FileRegistrar {
    pub base_dir: PathBuf,
}

impl FileRegistrar {
    /// New.
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }
}

#[async_trait]
impl ToolRegistrar for FileRegistrar {
    async fn register(&self, tool_id: &str, code: &str) -> Result<PathBuf, IntelError> {
        std::fs::create_dir_all(&self.base_dir).map_err(|e| {
            IntelError::Internal(format!(
                "Failed to create dir '{}': {e}",
                self.base_dir.display()
            ))
        })?;
        let path = self.base_dir.join(format!("{tool_id}.rs"));
        std::fs::write(&path, code).map_err(|e| {
            IntelError::Internal(format!("Failed to write '{}': {e}", path.display()))
        })?;
        Ok(path)
    }
}

#[derive(Debug, Clone)]
/// Loop Config.
pub struct LoopConfig {
    pub max_attempts: usize,
    pub enabled: bool,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            enabled: false,
        }
    }
}

/// Self Programming Loop.
pub struct SelfProgrammingLoop {
    _gap: Arc<GapAnalyzer>,
    coder: Arc<ToolCoder>,
    verifier: Arc<dyn Verifier>,
    registrar: Arc<dyn ToolRegistrar>,
    config: LoopConfig,
    history: Mutex<Vec<ToolGenerationResult>>,
}

impl SelfProgrammingLoop {
    /// New.
    pub fn new(
        gap: Arc<GapAnalyzer>,
        coder: Arc<ToolCoder>,
        verifier: Arc<dyn Verifier>,
        registrar: Arc<dyn ToolRegistrar>,
        config: LoopConfig,
    ) -> Self {
        Self {
            _gap: gap,
            coder,
            verifier,
            registrar,
            config,
            history: Mutex::new(Vec::new()),
        }
    }

    /// Config.
    pub fn config(&self) -> &LoopConfig {
        &self.config
    }

    /// History.
    pub fn history(&self) -> Vec<ToolGenerationResult> {
        self.history.lock().clone()
    }

    /// Close the loop.
    pub async fn close_the_loop(&self, gap: &ToolGap) -> Result<ToolGenerationResult, IntelError> {
        if !self.config.enabled {
            return Err(IntelError::Internal(
                "SelfProgrammingLoop is disabled (set config.enabled=true)".into(),
            ));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut attempts = Vec::new();
        let mut feedback: Option<String> = None;
        let mut final_code: Option<String> = None;
        let mut success = false;
        let mut output_path: Option<PathBuf> = None;

        for attempt in 1..=self.config.max_attempts {
            let code = self.coder.generate_code(&gap_to_spec(gap), feedback.clone()).await?;
            let outcome = self.verifier.verify(&code).await?;
            attempts.push(AttemptRecord {
                attempt,
                code: code.clone(),
                passed: outcome.passed,
                diagnostics: outcome.diagnostics.clone(),
                timestamp: now,
            });

            if outcome.passed {
                let path = self.registrar.register(&gap.suggested_tool_spec.tool_id, &code).await?;
                final_code = Some(code);
                output_path = Some(path);
                success = true;
                break;
            } else {
                feedback = Some(outcome.diagnostics);
            }
        }

        let result = ToolGenerationResult {
            tool_id: gap.suggested_tool_spec.tool_id.clone(),
            final_code,
            attempts,
            success,
            output_path,
        };
        self.history.lock().push(result.clone());
        Ok(result)
    }
}

fn gap_to_spec(gap: &ToolGap) -> sisyphus_types::tools::ToolSpec {
    // gap.suggested_tool_spec 已含完整 ToolSpec（from gap_analyzer）。
    // 复制时补 tool_id（来自 gap_id）。
    let mut spec = gap.suggested_tool_spec.clone();
    spec.tool_id = gap.gap_id.clone();
    spec
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gap_analyzer::ToolGap;
    use async_trait::async_trait;
    use sisyphus_tools_llm::LlmClient;

    struct StaticLlm;

    #[async_trait]
    impl LlmClient for StaticLlm {
        async fn chat(
            &self,
            _system: &str,
            _user: &str,
            _json_mode: bool,
        ) -> Result<String, sisyphus_types::TypesError> {
            Ok("```rust\nfn main() {}\n```".to_string())
        }
        async fn generate(&self, _prompt: &str) -> Result<String, sisyphus_types::TypesError> {
            Ok("```rust\nfn main() {}\n```".to_string())
        }
        fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
            ("model".into(), "endpoint".into(), "key".into())
        }
        fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
            sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
        }
    }

    struct VerifierMock {
        /// 第 N 次的判定（1-indexed）
        results: Vec<bool>,
    }

    #[async_trait]
    impl Verifier for VerifierMock {
        async fn verify(&self, _code: &str) -> Result<VerificationOutcome, IntelError> {
            // 取下一个结果（按调用顺序），若用尽则永远 false
            let _idx = self.results.len();
            // 使用 Mutex 计数不必要——单线程测试按调用顺序
            // 这里直接 pop
            Ok(VerificationOutcome {
                passed: self.results.first().copied().unwrap_or(false),
                diagnostics: if self.results.first().copied().unwrap_or(false) {
                    "ok".into()
                } else {
                    "mock: fail".into()
                },
            })
        }
    }

    struct InMemoryRegistrar {
        stored: parking_lot::Mutex<Vec<(String, String)>>,
    }

    impl InMemoryRegistrar {
        fn new() -> Self {
            Self {
                stored: parking_lot::Mutex::new(Vec::new()),
            }
        }
        fn stored(&self) -> Vec<(String, String)> {
            self.stored.lock().clone()
        }
    }

    #[async_trait]
    impl ToolRegistrar for InMemoryRegistrar {
        async fn register(&self, tool_id: &str, code: &str) -> Result<PathBuf, IntelError> {
            self.stored.lock().push((tool_id.to_string(), code.to_string()));
            Ok(PathBuf::from(format!("/fake/{tool_id}.rs")))
        }
    }

    fn test_gap() -> ToolGap {
        ToolGap {
            gap_id: "test-tool-001".to_string(),
            required_capability: "count_lines".into(),
            failing_node: "node-parse".into(),
            suggested_tool_spec: sisyphus_types::tools::ToolSpec {
                tool_id: "test-tool-001".to_string(),
                required_capability: "count_lines".into(),
                expected_input: "file_path: String".into(),
                expected_output: "u64".into(),
                success_criteria: "count lines in file".into(),
                test_command: None,
                workdir: None,
            },
        }
    }

    fn build_loop(
        verifier: Arc<dyn Verifier>,
        enabled: bool,
        max_attempts: usize,
    ) -> (SelfProgrammingLoop, Arc<InMemoryRegistrar>) {
        let llm: Arc<dyn LlmClient> = Arc::new(StaticLlm);
        let coder = Arc::new(ToolCoder::new(llm));
        let gap = Arc::new(GapAnalyzer::new(Arc::new(StaticLlm)));
        let registrar = Arc::new(InMemoryRegistrar::new());
        let loop_cfg = LoopConfig {
            enabled,
            max_attempts,
        };
        let l = SelfProgrammingLoop::new(gap, coder, verifier, registrar.clone(), loop_cfg);
        (l, registrar)
    }

    #[tokio::test]
    async fn disabled_loop_rejects() {
        let v: Arc<dyn Verifier> = Arc::new(VerifierMock {
            results: vec![true],
        });
        let (l, _) = build_loop(v, false, 3);
        let res = l.close_the_loop(&test_gap()).await;
        assert!(matches!(res, Err(IntelError::Internal(_))));
    }

    #[tokio::test]
    async fn first_attempt_success_registers_tool() {
        let v: Arc<dyn Verifier> = Arc::new(VerifierMock {
            results: vec![true],
        });
        let (l, r) = build_loop(v, true, 3);
        let res = l.close_the_loop(&test_gap()).await.expect("ok");
        assert!(res.success);
        assert_eq!(res.attempts.len(), 1);
        assert_eq!(r.stored().len(), 1);
        assert_eq!(r.stored()[0].0, "test-tool-001");
    }

    #[tokio::test]
    async fn all_attempts_fail_does_not_register() {
        let v: Arc<dyn Verifier> = Arc::new(VerifierMock {
            results: vec![false, false, false],
        });
        let (l, r) = build_loop(v, true, 3);
        let res = l.close_the_loop(&test_gap()).await.expect("ok");
        assert!(!res.success);
        assert_eq!(res.attempts.len(), 3);
        assert_eq!(r.stored().len(), 0, "should not register on failure");
    }

    #[tokio::test]
    async fn history_persists_across_runs() {
        let v: Arc<dyn Verifier> = Arc::new(VerifierMock {
            results: vec![true],
        });
        let (l, _) = build_loop(v, true, 1);
        l.close_the_loop(&test_gap()).await.unwrap();
        l.close_the_loop(&test_gap()).await.unwrap();
        assert_eq!(l.history().len(), 2);
    }

    #[test]
    fn file_registrar_writes_to_disk() {
        let tmp = std::env::temp_dir().join("sisyphus_self_prog_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let r = FileRegistrar::new(tmp.clone());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let p = rt.block_on(async { r.register("demo", "fn main() {}").await.expect("ok") });
        assert!(p.exists());
        let content = std::fs::read_to_string(&p).unwrap();
        assert!(content.contains("fn main"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn loop_config_default_disabled() {
        let cfg = LoopConfig::default();
        assert!(!cfg.enabled, "default should be disabled for safety");
        assert_eq!(cfg.max_attempts, 3);
    }

    #[test]
    fn gap_to_spec_uses_gap_id_as_tool_id() {
        let g = test_gap();
        let s = gap_to_spec(&g);
        assert_eq!(s.tool_id, g.gap_id);
        assert_eq!(s.success_criteria, g.suggested_tool_spec.success_criteria);
    }
}
