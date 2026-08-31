//! 合成器的单元测试，包括对抗式收敛测试，
//! 使用 mock LLM、执行器、状态存储、黄金数据集与评估器。

use super::*;
use crate::{ActionType, FlowSynthesizer};
use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fallback_decomposer() {
        let llm_decomposer = Arc::new(LlmGoalDecomposer::new(|_| Err("LLM Error".into())));
        let fallback = Arc::new(FallbackGoalDecomposer::new(llm_decomposer));
        let goal = "fetch and process";
        let actions = fallback.decompose(goal).await.unwrap();
        assert!(actions.contains(&ActionType::Fetch));
        assert!(actions.contains(&ActionType::Process));
    }

    #[tokio::test]
    async fn test_llm_decomposer_success() {
        let llm_decomposer = Arc::new(LlmGoalDecomposer::new(|_| {
            Ok(vec![ActionType::Fetch, ActionType::Sink])
        }));
        let goal = "some goal";
        let actions = llm_decomposer.decompose(goal).await.unwrap();
        assert_eq!(actions, vec![ActionType::Fetch, ActionType::Sink]);
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;
    use crate::simulation::MonteCarloSimulator;
    use sisyphus_types::ExecutionContext;
    use std::sync::Arc;

    struct MockLlm {
        call_count: std::sync::atomic::AtomicUsize,
        mode: std::sync::Arc<std::sync::Mutex<usize>>,
    }

    impl MockLlm {
        fn new() -> Self {
            Self {
                call_count: std::sync::atomic::AtomicUsize::new(0),
                mode: std::sync::Arc::new(std::sync::Mutex::new(0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl sisyphus_tools_llm::LlmClient for MockLlm {
        async fn chat(
            &self,
            _system: &str,
            prompt: &str,
            _stream: bool,
        ) -> Result<String, sisyphus_types::TypesError> {
            let _count = self.call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut mode = self.mode.lock().unwrap();

            if prompt.contains("Refine this design") {
                *mode += 1;
                Ok("Reasoning: Fixed the loop.\nJSON: {\"start_node\": 0, \"nodes\": [{ \"name\": \"Node0\", \"kind\": { \"Wasm\": { \"tool_id\": \"t1\", \"fuel_limit\": 1000 } } }], \"edges\": []}\nPlan: Fixed and robust.".to_string())
            } else if prompt.contains("Audit this design") {
                if *mode == 0 {
                    Ok("REJECTED: The path is too risky. Add a retry node.".into())
                } else {
                    Ok("APPROVED: The design is now robust.".into())
                }
            } else {
                Ok("Reasoning: Initial plan.\nJSON: {\"start_node\": 0, \"nodes\": [{ \"name\": \"Node0\", \"kind\": { \"Wasm\": { \"tool_id\": \"t1\", \"fuel_limit\": 1000 } } }], \"edges\": []}\nPlan: Initial strategy.".to_string())
            }
        }

        async fn generate(&self, _prompt: &str) -> Result<String, sisyphus_types::TypesError> {
            Ok("Mock generate".into())
        }

        fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
            (
                "default".to_string(),
                "default".to_string(),
                "default".to_string(),
            )
        }

        fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
            sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
        }
    }

    #[tokio::test]
    async fn test_adversarial_loop_convergence() {
        let llm = Arc::new(MockLlm::new());
        let executor = Arc::new(sisyphus_types::ExecutorRouter::new(Arc::new(MockExecutor)));
        let sim = Arc::new(MonteCarloSimulator::new(10));
        let _store = Arc::new(MockStateStore::new());

        let synthesizer = LogicalSOPSynthesizer::new(
            llm,
            executor,
            Arc::new(MockTestExecutor),
            sim,
            Arc::new(MockStateStore::new()),
            Arc::new(MockEvaluator::new()),
            Arc::new(MockGoldenDataset),
        );
        let result = synthesizer
            .synthesize(
                "test goal",
                &ExecutionContext::new(),
                "test",
                sisyphus_types::WorkMode::UltraWork,
                None,
            )
            .await;

        assert!(
            result.is_ok(),
            "Adversarial loop should converge to a valid SOP"
        );
        let res = result.unwrap();
        assert!(!res.plan_description.is_empty());
    }

    #[tokio::test]
    async fn test_strategic_memory_retrieval() {
        let llm = Arc::new(MockLlm::new());
        let executor = Arc::new(sisyphus_types::ExecutorRouter::new(Arc::new(MockExecutor)));
        let sim = Arc::new(MonteCarloSimulator::new(10));
        let _store = Arc::new(MockStateStore::new());

        let synthesizer = LogicalSOPSynthesizer::new(
            llm,
            executor,
            Arc::new(MockTestExecutor),
            sim,
            Arc::new(MockStateStore::new()),
            Arc::new(MockEvaluator::new()),
            Arc::new(MockGoldenDataset),
        );

        // 1. 第一次尝试：强制失败以保存教训
        let _ = synthesizer
            .synthesize(
                "memory goal",
                &ExecutionContext::new(),
                "test",
                sisyphus_types::WorkMode::UltraWork,
                None,
            )
            .await;

        // 2. 验证教训已保存
        let lessons = synthesizer.retrieve_lessons("memory goal").await;
        assert!(
            lessons.contains("SOP:"),
            "Should have saved a failed SOP as a lesson. Actual lessons: {}",
            lessons
        );

        // 3. 第二次尝试：验证仍可合成
        let res = synthesizer
            .synthesize(
                "memory goal",
                &ExecutionContext::new(),
                "test",
                sisyphus_types::WorkMode::UltraWork,
                None,
            )
            .await;
        assert!(res.is_ok());
    }

    struct MockTestExecutor;
    #[async_trait::async_trait]
    impl sisyphus_types::TestExecutor for MockTestExecutor {
        async fn run_test(
            &self,
            _command: &str,
            _workdir: Option<&str>,
        ) -> Result<sisyphus_types::TestResult, sisyphus_types::TypesError> {
            Ok(sisyphus_types::TestResult {
                success: true,
                stdout: "Mock Success".into(),
                stderr: "".into(),
                exit_code: 0,
                duration_ms: 10,
                error_summary: None,
            })
        }
    }

    struct MockExecutor;
    #[async_trait::async_trait]
    impl sisyphus_types::ToolExecutor for MockExecutor {
        async fn execute(
            &self,
            _: &str,
            _: Vec<u8>,
            _: u64,
        ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
            Ok(vec![])
        }
        async fn list_tools(
            &self,
        ) -> Result<Vec<sisyphus_types::ToolMetadata>, sisyphus_types::TypesError> {
            Ok(vec![])
        }
    }

    struct MockStateStore {
        data: std::sync::Mutex<std::collections::HashMap<String, serde_json::Value>>,
        experiences: std::sync::Mutex<Vec<sisyphus_types::experience::ExperienceTuple>>,
    }
    impl MockStateStore {
        fn new() -> Self {
            Self {
                data: std::sync::Mutex::new(std::collections::HashMap::new()),
                experiences: std::sync::Mutex::new(Vec::new()),
            }
        }
    }
    #[async_trait::async_trait]
    impl sisyphus_types::ExperienceRetriever for MockStateStore {
        async fn retrieve_relevant(
            &self,
            query: &str,
            _limit: usize,
        ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>, sisyphus_types::TypesError>
        {
            // Mock 简单检索：返回与查询匹配的经验
            let experiences = self.experiences.lock().unwrap();
            let filtered: Vec<_> =
                experiences.iter().filter(|e| e.context_text.contains(query)).cloned().collect();
            Ok(filtered)
        }
        async fn record_experience(
            &self,
            exp: sisyphus_types::experience::ExperienceTuple,
        ) -> Result<(), sisyphus_types::TypesError> {
            self.experiences.lock().unwrap().push(exp);
            Ok(())
        }
    }
    #[async_trait::async_trait]
    impl sisyphus_types::StateStore for MockStateStore {
        async fn get_state_json(
            &self,
            _project_id: &str,
            key: &str,
        ) -> Result<Option<serde_json::Value>, sisyphus_types::TypesError> {
            let res = self.data.lock().unwrap().get(key).cloned();
            println!("DEBUG: MockStateStore GET key: {}, result: {:?}", key, res);
            Ok(res)
        }
        async fn set_state_json(
            &self,
            _project_id: &str,
            key: &str,
            val: serde_json::Value,
        ) -> Result<(), sisyphus_types::TypesError> {
            println!("DEBUG: MockStateStore SET key: {}", key);
            self.data.lock().unwrap().insert(key.to_string(), val);
            Ok(())
        }
        async fn get(
            &self,
            _project_id: &str,
            key: &str,
        ) -> Result<Option<Vec<u8>>, sisyphus_types::TypesError> {
            let res = self.data.lock().unwrap().get(key).cloned();
            Ok(res.map(|v| serde_json::to_vec(&v).unwrap_or_default()))
        }
        async fn set(
            &self,
            _project_id: &str,
            key: &str,
            val: &[u8],
        ) -> Result<(), sisyphus_types::TypesError> {
            let v: serde_json::Value =
                serde_json::from_slice(val).unwrap_or(serde_json::Value::Null);
            self.data.lock().unwrap().insert(key.to_string(), v);
            Ok(())
        }
        async fn get_traces_for_sop(
            &self,
            _project_id: &str,
            _sop_id: &str,
        ) -> Result<Vec<(String, String, String, i64, i64)>, sisyphus_types::TypesError> {
            Ok(vec![])
        }
    }
}

#[cfg(test)]
struct MockGoldenDataset;
#[cfg(test)]
#[async_trait::async_trait]
impl sisyphus_types::GoldenDataset for MockGoldenDataset {
    async fn enroll(
        &self,
        _goal: &str,
        _config: sisyphus_types::SOPGraphConfig,
        _metric: f64,
    ) -> Result<(), sisyphus_types::TypesError> {
        Ok(())
    }
    async fn get_golden_sop(
        &self,
        _goal: &str,
    ) -> Result<Option<sisyphus_types::GoldenSop>, sisyphus_types::TypesError> {
        Ok(None)
    }
    async fn verify_no_regression(
        &self,
        _goal: &str,
        _candidate: &sisyphus_types::SOPGraphConfig,
        _current_metric: f64,
    ) -> Result<bool, sisyphus_types::TypesError> {
        Ok(true)
    }
}

#[cfg(test)]
struct MockEvaluator {
    call_count: std::sync::atomic::AtomicUsize,
}
#[cfg(test)]
impl MockEvaluator {
    fn new() -> Self {
        Self {
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}
#[cfg(test)]
#[async_trait::async_trait]
impl sisyphus_types::SopEvaluator for MockEvaluator {
    async fn evaluate(
        &self,
        _goal: &str,
        _config: &sisyphus_types::SOPGraphConfig,
    ) -> Result<sisyphus_types::SopEvaluation, sisyphus_types::TypesError> {
        let count = self.call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if count == 0 {
            // 第一次调用：拒绝以强制保存教训
            Ok(sisyphus_types::SopEvaluation {
                score: 0.0,
                metrics: std::collections::HashMap::new(),
                feedback: "Rejected for testing".into(),
                is_acceptable: false,
            })
        } else {
            // 后续调用：接受
            Ok(sisyphus_types::SopEvaluation {
                score: 1.0,
                metrics: std::collections::HashMap::new(),
                feedback: "Perfect".into(),
                is_acceptable: true,
            })
        }
    }
}
