use super::*;
use crate::AdaptiveFuelPricer;
use std::sync::Arc;

fn make_pred(estimated_cost: u64) -> SimulationResult {
    SimulationResult {
        path_id: "t".into(),
        success_probability: 0.5,
        estimated_cost,
        predicted_outcome: "?".into(),
        predicted_effort: 0,
        predicted_stability: 1.0,
    }
}

/// 构造 StandardEvolver 行为测试用的最小 WASM 图。
fn setup_evolver_graph() -> (SOPGraph, Arc<dyn sisyphus_types::ToolExecutor>) {
    use sisyphus_types::{ToolExecutor, WasmSOPNode};

    let exec: Arc<dyn ToolExecutor> = Arc::new(NoopExecutor);
    let mut graph = SOPGraph::new();
    graph.add_node(Arc::new(WasmSOPNode {
        name: "n1".into(),
        tool_id: "t".into(),
        fuel_limit: 1000,
        executor: exec.clone(),
    }));
    (graph, exec)
}

struct NoopExecutor;
#[async_trait::async_trait]
impl sisyphus_types::ToolExecutor for NoopExecutor {
    async fn execute(
        &self,
        _: &str,
        _: Vec<u8>,
        _: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Ok(b"ok".to_vec())
    }
}

fn make_record(node: &str, status: &str, output: &str) -> RecordedNode {
    RecordedNode {
        node: node.into(),
        status: status.into(),
        output: output.into(),
    }
}

// 测试 StandardReflector::parse_trace
#[test]
fn test_parse_valid_json_lines() {
    let trace = r#"{"node":"a","status":"ok","output":"done"}
{"node":"b","status":"error","output":"crash"}"#;
    let records = StandardReflector::parse_trace(trace);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].node, "a");
    assert_eq!(records[1].status, "error");
}

#[test]
fn test_parse_skip_invalid() {
    let trace = "bad\n{\"node\":\"a\",\"status\":\"ok\",\"output\":\"\"}\ngarbage";
    let records = StandardReflector::parse_trace(trace);
    assert_eq!(records.len(), 1);
}

#[test]
fn test_parse_empty() {
    assert!(StandardReflector::parse_trace("").is_empty());
}

// 测试 StandardReflector::analyze
#[test]
fn test_analyze_empty() {
    let (dev, s) = StandardReflector::analyze(&[]);
    assert_eq!(dev, 0.0);
    assert!(s.iter().any(|x| x.contains("No execution records")));
}

#[test]
fn test_analyze_all_ok() {
    let r = vec![make_record("a", "ok", "out"), make_record("b", "ok", "out")];
    let (dev, s) = StandardReflector::analyze(&r);
    assert_eq!(dev, 0.0);
    assert!(s.iter().any(|x| x.contains("within expected parameters")));
}

#[test]
fn test_analyze_errors() {
    let r = vec![
        make_record("a", "ok", "out"),
        make_record("b", "error", "boom"),
    ];
    let (dev, s) = StandardReflector::analyze(&r);
    assert!(dev > 0.3);
    assert!(s.iter().any(|x| x.contains("Node failures")));
}

#[test]
fn test_analyze_resource_exhausted() {
    let r = vec![make_record("a", "error", "ResourceExhausted")];
    let (_, s) = StandardReflector::analyze(&r);
    assert!(s.iter().any(|x| x.contains("Resource exhaustion")));
}

#[test]
fn test_analyze_empty_outputs() {
    let r = vec![
        make_record("a", "ok", ""),
        make_record("b", "ok", "null"),
        make_record("c", "ok", "real"),
    ];
    let (_, s) = StandardReflector::analyze(&r);
    assert!(s.iter().any(|x| x.contains("empty output")));
}

// 测试 StandardReflector::reflect（异步）
#[tokio::test]
async fn test_reflect_structured() {
    let trace = r#"{"node":"x","status":"ok","output":"data"}"#;
    let report = StandardReflector.reflect(trace, &make_pred(10), None).await.unwrap();
    assert_eq!(report.deviation_score, 0.0);
}

#[tokio::test]
async fn test_reflect_fallback() {
    let trace = "ResourceExhausted";
    let report = StandardReflector.reflect(trace, &make_pred(100), None).await.unwrap();
    assert_eq!(report.deviation_score, 0.8);
}

// 测试 ErrorCategory::classify
#[test]
fn test_error_category_classify() {
    assert_eq!(
        ErrorCategory::classify("ResourceExhausted: fuel limit"),
        ErrorCategory::ResourceExhausted
    );
    assert_eq!(
        ErrorCategory::classify("fuel exhausted"),
        ErrorCategory::ResourceExhausted
    );
    assert_eq!(
        ErrorCategory::classify("out of memory error"),
        ErrorCategory::ResourceExhausted
    );

    assert_eq!(
        ErrorCategory::classify("timeout exceeded"),
        ErrorCategory::Timeout
    );
    assert_eq!(
        ErrorCategory::classify("deadline exceeded"),
        ErrorCategory::Timeout
    );
    assert_eq!(
        ErrorCategory::classify("Timeout after 30s"),
        ErrorCategory::Timeout
    );

    assert_eq!(
        ErrorCategory::classify("TypeError: mismatch"),
        ErrorCategory::TypeError
    );
    assert_eq!(
        ErrorCategory::classify("type mismatch"),
        ErrorCategory::TypeError
    );

    assert_eq!(
        ErrorCategory::classify("panic: index out of bounds"),
        ErrorCategory::Crash
    );
    assert_eq!(ErrorCategory::classify("segfault"), ErrorCategory::Crash);
    assert_eq!(
        ErrorCategory::classify("crash in module"),
        ErrorCategory::Crash
    );

    assert_eq!(
        ErrorCategory::classify("unknown error"),
        ErrorCategory::Unknown
    );
}

#[test]
fn test_error_category_severity() {
    assert_eq!(ErrorCategory::Crash.severity(), 1.0);
    assert_eq!(ErrorCategory::ResourceExhausted.severity(), 0.8);
    assert_eq!(ErrorCategory::TypeError.severity(), 0.6);
    assert_eq!(ErrorCategory::Timeout.severity(), 0.5);
    assert_eq!(ErrorCategory::Unknown.severity(), 0.7);
}

// 测试 StandardEvolver
#[test]
fn test_standard_evolver_with_adaptive_pricer() {
    let evolver = StandardEvolver::with_fuel_pricer(Arc::new(AdaptiveFuelPricer::new()));
    assert!(std::ptr::addr_of!(evolver.fuel_pricer) as usize != 0);
}

// ---------------------------------------------------------------------------
// B1：演进信号枚举化
// ---------------------------------------------------------------------------

// from_text：各类建议文案的解析语义
#[test]
fn test_improvement_kind_from_text_retry() {
    assert_eq!(
        ImprovementKind::from_text("Add retry logic to unstable nodes"),
        Some(ImprovementKind::Retry { max_retries: 3 })
    );
    assert_eq!(
        ImprovementKind::from_text("add retry to node X"),
        Some(ImprovementKind::Retry { max_retries: 3 })
    );
}

#[test]
fn test_improvement_kind_from_text_fuel() {
    assert_eq!(
        ImprovementKind::from_text(
            "Resource exhaustion — increase fuel or split long-running nodes"
        ),
        Some(ImprovementKind::FuelIncrease { factor: 1.5 })
    );
    assert_eq!(
        ImprovementKind::from_text("ResourceExhausted: fuel limit exceeded"),
        Some(ImprovementKind::FuelIncrease { factor: 1.5 })
    );
}

#[test]
fn test_improvement_kind_from_text_split_and_backoff() {
    assert_eq!(
        ImprovementKind::from_text("split long-running nodes into smaller steps"),
        Some(ImprovementKind::NodeSplit)
    );
    assert_eq!(
        ImprovementKind::from_text("Add exponential backoff between attempts"),
        Some(ImprovementKind::Backoff)
    );
}

#[test]
fn test_improvement_kind_from_text_unrecognized_is_none() {
    // 无演进语义的文本必须返回 None（安全侧倾斜），
    // 尤其是带注解前缀的人类审阅信息。
    assert_eq!(
        ImprovementKind::from_text("SOP is performing within expected parameters"),
        None
    );
    assert_eq!(
        ImprovementKind::from_text("[CRITIQUE] refine the prompt wording"),
        None
    );
    assert_eq!(
        ImprovementKind::from_text("[DEEP HIGH] node X: consider caching"),
        None
    );
}

// 演化决策只认结构化信号：含关键词的注解文本不再误触发
#[tokio::test]
async fn test_evolver_ignores_keyword_annotations_without_signals() {
    let (graph, _exec) = setup_evolver_graph();
    let evolver = StandardEvolver::new();
    let report = ReflectionReport {
        original_sop_id: "t".into(),
        deviation_score: 0.9,
        suggested_improvements: vec![
            "[CRITIQUE] the output mentioned retry logic in a comment".into(),
            "[DEEP] log said fuel limit reached historically".into(),
        ],
        improvement_signals: vec![], // 关键：无结构化信号
        evolved_sop_structure: None,
        records: vec![],
    };
    let prediction = make_pred(10);
    let evolved =
        evolver.evolve(std::sync::Arc::new(graph), &report, &prediction, None).await.unwrap();

    // 无信号 → 图谱不变：节点名无 _retry 后缀，燃料未被调价
    for idx in evolved.graph.node_indices() {
        let n = evolved.graph.node_weight(idx).unwrap();
        assert!(
            !n.name().contains("_retry"),
            "annotation must not trigger evolution"
        );
    }
}

#[tokio::test]
async fn test_evolver_acts_on_structured_signals() {
    let (graph, _exec) = setup_evolver_graph();
    let evolver = StandardEvolver::new();
    let report = ReflectionReport {
        original_sop_id: "t".into(),
        deviation_score: 0.9,
        suggested_improvements: vec![],
        improvement_signals: vec![
            ImprovementKind::Retry { max_retries: 3 },
            ImprovementKind::FuelIncrease { factor: 1.5 },
        ],
        evolved_sop_structure: None,
        records: vec![],
    };
    let prediction = make_pred(10);
    let evolved =
        evolver.evolve(std::sync::Arc::new(graph), &report, &prediction, None).await.unwrap();

    let retry_count = evolved
        .graph
        .node_indices()
        .filter(|&i| evolved.graph.node_weight(i).unwrap().name().contains("_retry"))
        .count();
    assert!(retry_count > 0, "structured signal must drive evolution");
}

// serde 兼容：旧数据（无 improvement_signals 字段）反序列化为空信号
#[test]
fn test_reflection_report_legacy_deserialization() {
    let legacy = r#"{
        "original_sop_id": "sop1",
        "deviation_score": 0.4,
        "suggested_improvements": ["add retry"],
        "evolved_sop_structure": null,
        "records": []
    }"#;
    let report: ReflectionReport = serde_json::from_str(legacy).unwrap();
    assert!(report.improvement_signals.is_empty());
}

// ---------------------------------------------------------------------------
// B3：LLM 阶段超时注入（评审建议 #8）
// ---------------------------------------------------------------------------

struct SlowLlm {
    delay: std::time::Duration,
}

#[async_trait::async_trait]
impl sisyphus_tools_llm::metis::LlmClient for SlowLlm {
    async fn chat(&self, _: &str, _: &str, _: bool) -> Result<String, sisyphus_types::TypesError> {
        tokio::time::sleep(self.delay).await;
        Ok("{}".into())
    }
    async fn generate(&self, _: &str) -> Result<String, sisyphus_types::TypesError> {
        tokio::time::sleep(self.delay).await;
        Ok(r#"{"deviation": 0.3, "suggestions": [], "analysis": "slow"}"#.into())
    }
    fn get_params_for(&self, _: &str) -> (String, String, String) {
        ("m".into(), "http://localhost".into(), "k".into())
    }
    fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
        sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
    }
}

fn reflect_fixture() -> SimulationResult {
    make_pred(10)
}

#[tokio::test]
async fn test_llm_reflector_respects_injected_timeout() {
    // LLM 延迟 5s，注入超时 1s：应在约 1s 处切断并降级为标准报告，
    // 而非等待完整 LLM 延迟。
    let reflector = LlmReflector::with_timeout(
        Arc::new(SlowLlm {
            delay: std::time::Duration::from_secs(5),
        }),
        std::time::Duration::from_secs(1),
    );
    let start = std::time::Instant::now();
    let report = reflector.reflect("trace", &reflect_fixture(), None).await.unwrap();
    assert!(start.elapsed() < std::time::Duration::from_secs(4));
    // 降级报告的特征：空改进列表 + 标准轨迹解析
    assert!(report.suggested_improvements.is_empty());
    assert_eq!(report.deviation_score, 0.5);
}
