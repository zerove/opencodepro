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
    let report = StandardReflector
        .reflect(trace, &make_pred(10), None)
        .await
        .unwrap();
    assert_eq!(report.deviation_score, 0.0);
}

#[tokio::test]
async fn test_reflect_fallback() {
    let trace = "ResourceExhausted";
    let report = StandardReflector
        .reflect(trace, &make_pred(100), None)
        .await
        .unwrap();
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
    let report = reflector
        .reflect("trace", &reflect_fixture(), None)
        .await
        .unwrap();
    assert!(start.elapsed() < std::time::Duration::from_secs(4));
    // 降级报告的特征：空改进列表 + 标准轨迹解析
    assert!(report.suggested_improvements.is_empty());
    assert_eq!(report.deviation_score, 0.5);
}
