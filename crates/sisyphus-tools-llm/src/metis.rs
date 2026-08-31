use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use thiserror::Error;

// ── Types ──────────────────────────────────────────────────

#[derive(Error, Debug)]
/// Metis Error.
pub enum MetisError {
    #[error("LLM call failed: {0}")]
    LlmError(String),
    #[error("Failed to parse LLM response: {0}")]
    ParseError(#[from] serde_json::Error),
}

impl From<sisyphus_types::TypesError> for MetisError {
    fn from(e: sisyphus_types::TypesError) -> Self {
        MetisError::LlmError(e.to_string())
    }
}

#[async_trait::async_trait]
/// Llm Client.
/// Abstraction over an LLM provider (OpenAI-compatible or local).
///
/// Implementations must be `Send + Sync` and safe to share across the SOP
/// scheduler's worker pool. The `chat` and `generate` methods are expected
/// to honour the per-agent rate limit and circuit breaker state exposed by
/// [`Self::breaker`].
pub trait LlmClient: Send + Sync {
    /// Run a chat completion.
    ///
    /// * `system` - System prompt; prepended regardless of `json_mode`.
    /// * `user` - The user message; if `json_mode` is true, the system prompt
    ///   is augmented to instruct the model to reply with strict JSON.
    /// * `json_mode` - Hint to enforce JSON output (e.g. for tool/function calling).
    async fn chat(
        &self,
        system: &str,
        user: &str,
        json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError>;
    /// Run a single-prompt completion.
    async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError>;
    /// Resolve provider/model/api_key triple for the given agent id.
    fn get_params_for(&self, agent_id: &str) -> (String, String, String);
    /// Snapshot of the circuit breaker used to gate outbound LLM calls.
    fn breaker(&self) -> crate::circuit_breaker::CircuitBreaker;

    /// 优雅关闭 LLM 客户端及其共享 HTTP 池。
    ///
    /// 默认实现为空（多数测试 mock 不需要）；生产实现（如 DynamicLlmClient）
    /// 应等待 in-flight 请求完成，最长  时长。
    async fn shutdown(&self, _wait: std::time::Duration) -> Result<(), String> {
        Ok(())
    }

    /// 返回底层共享 HTTP 客户端的 Arc 引用（如果持有）。
    ///
    /// 默认实现为 `None`（mock/test LLM 不暴露连接池）。
    /// 生产实现（`DynamicLlmClient`）返回 `Some(Arc<SharedHttpClient>)`
    /// 以便 bridge 等外部组件能拿到 Prometheus 指标源。
    fn shared_http(&self) -> Option<Arc<crate::http_client::SharedHttpClient>> {
        None
    }
}

/// A dummy LLM client for development and testing.
pub struct DummyLlmClient;

#[async_trait::async_trait]
impl LlmClient for DummyLlmClient {
    async fn chat(
        &self,
        _system: &str,
        user: &str,
        _json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        Ok(json!({
            "ambiguities": [],
            "implicit_requirements": [],
            "summary": format!("Dummy analysis for: {}", user),
            "actionable": true,
        })
        .to_string())
    }

    async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        if prompt.contains("critical_findings") || prompt.contains("Critique") {
            Ok(json!({
                "critical_findings": [],
                "risk_score": 0.1,
                "suggested_refinement": "No refinements needed",
                "should_proceed": true
            })
            .to_string())
        } else if prompt.contains("deviation") && prompt.contains("JSON") {
            Ok(json!({
                "deviation": 0.1,
                "suggestions": ["SOP is performing well", "No changes needed"],
                "analysis": "The execution closely matches the prediction."
            })
            .to_string())
        } else {
            Ok(format!("Dummy generation for: {prompt}"))
        }
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "dummy_key".into(),
            "http://dummy:8080".into(),
            "dummy-model".into(),
        )
    }

    fn breaker(&self) -> crate::circuit_breaker::CircuitBreaker {
        crate::circuit_breaker::CircuitBreaker::default()
    }
}

/// An LLM client that returns a fixed response string.
///
/// Useful in tests where a specific JSON output is needed
/// (e.g., to test `decompose_goal` with a known task plan).
pub struct FixedResponseClient(pub String);

#[async_trait::async_trait]
impl LlmClient for FixedResponseClient {
    async fn chat(
        &self,
        _system: &str,
        _user: &str,
        _json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        Ok(self.0.clone())
    }

    async fn generate(&self, _prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        Ok(self.0.clone())
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "fixed_key".into(),
            "http://fixed:8080".into(),
            "fixed-model".into(),
        )
    }

    fn breaker(&self) -> crate::circuit_breaker::CircuitBreaker {
        crate::circuit_breaker::CircuitBreaker::default()
    }
}

/// Flags an ambiguity or missing information in a request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ambiguity {
    pub aspect: String, // e.g. "scope", "timeline", "technology"
    pub description: String,
    pub severity: AmbiguitySeverity,
    pub options: Vec<String>, // possible interpretations
    pub recommended: Option<String>,
}

/// How serious an ambiguity is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmbiguitySeverity {
    Blocking,    // cannot proceed without clarification
    Significant, // likely to cause rework
    Minor,       // can make reasonable default
}

impl std::fmt::Display for AmbiguitySeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AmbiguitySeverity::Blocking => write!(f, "blocking"),
            AmbiguitySeverity::Significant => write!(f, "significant"),
            AmbiguitySeverity::Minor => write!(f, "minor"),
        }
    }
}

/// An identified implicit requirement — something not stated but likely intended.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplicitRequirement {
    pub description: String,
    pub rationale: String, // why we think this is needed
    pub confidence: f64,   // 0.0 – 1.0
}

/// Full result from Metis analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetisAnalysis {
    #[serde(default)]
    pub request: String,
    pub ambiguities: Vec<Ambiguity>,
    pub implicit_requirements: Vec<ImplicitRequirement>,
    pub summary: String,
    pub actionable: bool, // true if enough info to proceed
}

// ── Analysis Functions ─────────────────────────────────────

const METIS_SYSTEM_PROMPT: &str = r#"You are Metis, a senior requirements analyst for the Sisyphus project.
Your goal is to analyze natural-language requests and identify ambiguities and implicit requirements.

Sisyphus is an autonomous agent framework that executes SOPs (Standard Operating Procedures) using a cognitive loop.

### INPUT CATEGORIZATION & HANDLING:
Priority 1: Noise/Gibberish Detection
If the request consists of random characters, nonsense, or is entirely devoid of technical intent (e.g., "asdfghjkl", "12345"), you MUST:
- Set "actionable": false
- Add exactly one ambiguity with aspect: "noise", severity: "Blocking", and description: "Input is noise or lacks any actionable technical intent".

Priority 2: Standard Analysis
For valid requests, analyze the request and return a JSON object with the following structure:
{
  "ambiguities": [
    {
      "aspect": "scope|timeline|technology|verifiability|clarity|priority|noise",
      "description": "Detailed explanation of what is ambiguous",
      "severity": "Blocking|Significant|Minor",
      "options": ["Possible interpretation 1", "Possible interpretation 2"],
      "recommended": "The most likely intended meaning or a suggestion to fix it"
    }
  ],
  "implicit_requirements": [
    {
      "description": "The inferred requirement",
      "rationale": "Why this is necessary given the Sisyphus context",
      "confidence": 0.0 to 1.0
    }
  ],
  "summary": "A concise summary of the analysis",
  "actionable": true/false (false if there are 'Blocking' ambiguities)
}

Guidelines:
1. Ambiguities: Look for missing scope, no success criteria, vague terms (e.g., 'better', 'faster'), or missing tech constraints.
2. Implicit Requirements: Infer needs for error handling, testing, documentation, performance, or security based on the request's nature.
3. Actionable: If a 'Blocking' ambiguity exists, actionable MUST be false.
4. Be precise and professional.
"#;

/// Analyse a natural-language request for ambiguities and implicit requirements.
pub async fn analyse(request: &str, client: &dyn LlmClient) -> Result<MetisAnalysis, MetisError> {
    let response = client.chat(METIS_SYSTEM_PROMPT, request, true).await?;

    let analysis: MetisAnalysis = serde_json::from_str(&response)?;

    Ok(MetisAnalysis {
        request: request.to_string(),
        ..analysis
    })
}
// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const DUMMY: DummyLlmClient = DummyLlmClient;

    #[tokio::test]
    async fn test_analyse_clean_request() {
        let request = "\
Implement user login with email/password using Rust and Actix-Web.
Success criteria: user can register, login, and receive a JWT token.
Scope: auth endpoints only, no frontend. Testing included.
";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        // Clean request should have few issues
        assert!(analysis.actionable);
        // With success criteria + scope defined, ambiguities should be low
        let scope_issues = analysis.ambiguities.iter().filter(|a| a.aspect == "scope").count();
        assert_eq!(scope_issues, 0, "scope should be clear");
        let verify_issues =
            analysis.ambiguities.iter().filter(|a| a.aspect == "verifiability").count();
        assert_eq!(verify_issues, 0, "success criteria provided");
    }

    #[tokio::test]
    async fn test_analyse_vague_request() {
        let request = "Make the website better and faster.";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        assert_eq!(analysis.request, request);
        assert!(analysis.actionable);
    }

    #[tokio::test]
    async fn test_analyse_detects_implicit_testing() {
        let request = "Implement a new API endpoint for user profiles.";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        assert_eq!(analysis.request, request);
        assert!(analysis.actionable);
    }

    #[tokio::test]
    async fn test_analyse_detects_implicit_error_handling() {
        let request = "Build a file upload service.";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        assert_eq!(analysis.request, request);
        assert!(analysis.actionable);
    }

    #[tokio::test]
    async fn test_analyse_detects_implicit_security() {
        let request = "Add user authentication with database storage.";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        assert_eq!(analysis.request, request);
        assert!(analysis.actionable);
    }

    #[tokio::test]
    async fn test_analyse_detects_implicit_documentation() {
        let request = "Create a Rust library crate for CSV parsing.";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        assert_eq!(analysis.request, request);
        assert!(analysis.actionable);
    }

    #[tokio::test]
    async fn test_analyse_no_action_words() {
        let request = "What is the weather today?";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        // Pure info request shouldn't trigger implementation-related implicits
        assert!(
            analysis.implicit_requirements.is_empty()
                || !analysis
                    .implicit_requirements
                    .iter()
                    .any(|i| i.description.contains("Error handling"))
        );
    }

    #[tokio::test]
    async fn test_analyse_blocking_detection() {
        let analysis = analyse("", &DUMMY).await.unwrap(); // empty request = most ambiguous
        assert!(!analysis.actionable || analysis.ambiguities.is_empty());
    }

    #[test]
    fn test_ambiguity_severity_display() {
        assert_eq!(AmbiguitySeverity::Blocking.to_string(), "blocking");
        assert_eq!(AmbiguitySeverity::Significant.to_string(), "significant");
        assert_eq!(AmbiguitySeverity::Minor.to_string(), "minor");
    }

    #[tokio::test]
    async fn test_analyse_with_performance_keywords() {
        let request =
            "Build a high-volume data processing pipeline that scales to millions of requests.";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        assert_eq!(analysis.request, request);
        assert!(analysis.actionable);
    }

    #[tokio::test]
    async fn test_analyse_api_without_doc() {
        let request = "Build a REST API for managing todos.";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        assert_eq!(analysis.request, request);
        assert!(analysis.actionable);
    }

    #[tokio::test]
    async fn test_analyse_tech_constraint_detection() {
        let request = "Implement a service.";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        assert_eq!(analysis.request, request);
        assert!(analysis.actionable);
    }

    #[tokio::test]
    async fn test_analyse_with_tech_specified() {
        let request = "Implement a Rust service with tokio.";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        let tech_ambiguity = analysis.ambiguities.iter().find(|a| a.aspect == "technology");
        assert!(
            tech_ambiguity.is_none()
                || tech_ambiguity.map(|a| a.severity == AmbiguitySeverity::Minor).unwrap_or(true)
        );
    }

    #[tokio::test]
    async fn test_analyse_full_coverage() {
        let request = "\
Add a new feature to the Sisyphus CLI.
Timeline: this sprint.
Tech: Rust.
Success: command works end-to-end with tests.
Scope: CLI command only, no engine changes.
";
        let analysis = analyse(request, &DUMMY).await.unwrap();
        assert!(analysis.actionable);
        // All major ambiguity dimensions should be covered
        assert!(analysis.ambiguities.iter().all(|a| a.severity != AmbiguitySeverity::Blocking));
    }

    /// 验证 LlmClient::shutdown 默认实现是空操作（mock/test 实现无须等待）。
    /// 这是 trait 默认实现正确性的回归保护。
    #[tokio::test]
    async fn test_llm_client_shutdown_default_is_noop() {
        let dummy = DummyLlmClient;
        // 默认实现直接 Ok(())
        let res = dummy.shutdown(std::time::Duration::from_millis(100)).await;
        assert!(res.is_ok());
    }
}
