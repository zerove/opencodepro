use serde::{Deserialize, Serialize};
use crate::metis::LlmClient;
use crate::ToolResult;

// ── Types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecuritySeverity {
    Critical,
    High,
    Medium,
    Low,
}

impl std::fmt::Display for SecuritySeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecuritySeverity::Critical => write!(f, "Critical"),
            SecuritySeverity::High => write!(f, "High"),
            SecuritySeverity::Medium => write!(f, "Medium"),
            SecuritySeverity::Low => write!(f, "Low"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vulnerability {
    pub vuln_type: String,       // e.g. "Buffer Overflow", "SQL Injection"
    pub severity: SecuritySeverity,
    pub line: Option<usize>,
    pub message: String,
    pub fix: String,             // Actionable fix code or instructions
    pub cve_cwe: Option<String>, // CVE or CWE reference
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAuditReport {
    pub summary: String,
    #[serde(default)]
    pub vulnerabilities: Vec<Vulnerability>,
    #[serde(default)]
    pub passed: bool,            // true if no Critical or High vulnerabilities exist
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackVector {
    pub vector: String,
    pub description: String,
    pub impact: String,
    pub mitigation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatModelReport {
    pub summary: String,
    pub attack_vectors: Vec<AttackVector>,
    pub overall_risk: SecuritySeverity,
}

// ── Prompts ─────────────────────────────────────────────────

const SECURITY_SYSTEM_PROMPT: &str = r#"You are SecurityOracle, a world-class security researcher and auditor for the Sisyphus project.
Your goal is to identify security vulnerabilities in code and architecture, providing professional, actionable, and precise reports.

You must audit code from the following dimensions:
1. Memory Safety (Rust Specific):
   - Audit `unsafe` blocks for undefined behavior.
   - Detect potential deadlocks, race conditions, or improper synchronization.
   - Check for integer overflows/underflows in critical calculations.

2. Injection Vulnerabilities:
   - Detect SQL injection, Command injection, XSS, or Log injection.
   - Verify that all external inputs are strictly validated and sanitized.

3. Access Control & Data Privacy:
   - Identify broken authentication or authorization logic.
   - Check for privilege escalation paths or insecure direct object references (IDOR).
   - Detect sensitive data leaks (PII, secrets, keys) in logs or responses.

4. Protocol & Network Security:
   - Audit gRPC/TCP implementations for missing encryption (TLS).
   - Check for inadequate timeouts or lack of rate limiting (DoS risks).
   - Verify protocol state machine robustness against malformed packets.

Output Format:
You MUST return a JSON object. For code audits, use the `SecurityAuditReport` structure:
{
  "summary": "High-level security posture summary",
  "vulnerabilities": [
    {
      "vuln_type": "Type of vulnerability",
      "severity": "Critical|High|Medium|Low",
      "line": 123,
      "message": "Detailed explanation of the flaw",
      "fix": "Concrete code fix or specific implementation steps",
      "cve_cwe": "CWE-XXX or CVE-XXXX-XXXX"
    }
  ],
  "passed": true/false (false if any Critical or High vulnerabilities exist)
}

For threat modeling, use the `ThreatModelReport` structure:
{
  "summary": "Overall architectural security analysis",
  "attack_vectors": [
    {
      "vector": "Name of the attack vector",
      "description": "How the attack is performed",
      "impact": "What the attacker gains",
      "mitigation": "How to defend against it"
    }
  ],
  "overall_risk": "Critical|High|Medium|Low"
}

Guidelines:
- Be precise. Do not give generic advice.
- Provide actual code snippets for fixes.
- Reference CWE/CVE where applicable.
- If no vulnerabilities are found, set `passed: true` and provide a summary of why the code is secure.
"#;

// ── Core Functions ──────────────────────────────────────────

/// Audit source code for security vulnerabilities.
pub async fn audit_code(client: &dyn LlmClient, path: &std::path::Path, content: &str) -> ToolResult<SecurityAuditReport> {
    let user_prompt = format!("File: {}\n\nContent:\n{}", path.to_string_lossy(), content);
    let response = client
        .chat(SECURITY_SYSTEM_PROMPT, &user_prompt, true)
        .await
        .map_err(|e| format!("LLM call failed: {e}"))?;

    let report: SecurityAuditReport = serde_json::from_str(&response)
        .map_err(|e| format!("Failed to parse security report: {e}"))?;

    Ok(report)
}

/// Analyze architecture description for potential attack vectors.
pub async fn threat_model(client: &dyn LlmClient, arch_text: &str) -> ToolResult<ThreatModelReport> {
    let response = client
        .chat(SECURITY_SYSTEM_PROMPT, arch_text, true)
        .await
        .map_err(|e| format!("LLM call failed: {e}"))?;

    let report: ThreatModelReport = serde_json::from_str(&response)
        .map_err(|e| format!("Failed to parse threat model: {e}"))?;

    Ok(report)
}
