use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_tools_core::ToolResult;
use serde::{Deserialize, Serialize};

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
    pub vuln_type: String, // e.g. "Buffer Overflow", "SQL Injection"
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
    pub passed: bool, // true if no Critical or High vulnerabilities exist
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
    #[serde(default)]
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
pub async fn audit_code(
    client: &dyn LlmClient,
    path: &std::path::Path,
    content: &str,
) -> ToolResult<SecurityAuditReport> {
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
pub async fn threat_model(
    client: &dyn LlmClient,
    arch_text: &str,
) -> ToolResult<ThreatModelReport> {
    let response = client
        .chat(SECURITY_SYSTEM_PROMPT, arch_text, true)
        .await
        .map_err(|e| format!("LLM call failed: {e}"))?;

    let report: ThreatModelReport = serde_json::from_str(&response)
        .map_err(|e| format!("Failed to parse threat model: {e}"))?;

    Ok(report)
}

// ── Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── SecuritySeverity ──────────────────────────────────

    #[test]
    fn test_severity_display() {
        assert_eq!(SecuritySeverity::Critical.to_string(), "Critical");
        assert_eq!(SecuritySeverity::High.to_string(), "High");
        assert_eq!(SecuritySeverity::Medium.to_string(), "Medium");
        assert_eq!(SecuritySeverity::Low.to_string(), "Low");
    }

    #[test]
    fn test_severity_equality() {
        assert_eq!(SecuritySeverity::Critical, SecuritySeverity::Critical);
        assert_ne!(SecuritySeverity::High, SecuritySeverity::Low);
    }

    #[test]
    fn test_severity_serde_roundtrip() {
        let cases = [
            SecuritySeverity::Critical,
            SecuritySeverity::High,
            SecuritySeverity::Medium,
            SecuritySeverity::Low,
        ];
        for severity in &cases {
            let json = serde_json::to_string(severity).unwrap();
            let deserialized: SecuritySeverity = serde_json::from_str(&json).unwrap();
            assert_eq!(*severity, deserialized);
        }
    }

    // ── Vulnerability ────────────────────────────────────

    #[test]
    fn test_vulnerability_construction() {
        let vuln = Vulnerability {
            vuln_type: "Buffer Overflow".into(),
            severity: SecuritySeverity::High,
            line: Some(42),
            message: "Unsafe buffer access".into(),
            fix: "Use safe abstractions".into(),
            cve_cwe: Some("CWE-119".into()),
        };
        assert_eq!(vuln.vuln_type, "Buffer Overflow");
        assert_eq!(vuln.severity, SecuritySeverity::High);
        assert_eq!(vuln.line, Some(42));
        assert!(vuln.cve_cwe.is_some());
    }

    #[test]
    fn test_vulnerability_serde_roundtrip() {
        let vuln = Vulnerability {
            vuln_type: "SQL Injection".into(),
            severity: SecuritySeverity::Critical,
            line: None,
            message: "Raw SQL query".into(),
            fix: "Use parameterized queries".into(),
            cve_cwe: None,
        };
        let json = serde_json::to_string_pretty(&vuln).unwrap();
        let deserialized: Vulnerability = serde_json::from_str(&json).unwrap();
        assert_eq!(vuln.vuln_type, deserialized.vuln_type);
        assert_eq!(vuln.severity, deserialized.severity);
        assert_eq!(vuln.line, deserialized.line);
        assert_eq!(vuln.cve_cwe, deserialized.cve_cwe);
    }

    // ── SecurityAuditReport ───────────────────────────────

    #[test]
    fn test_audit_report_default_passed_true() {
        let report = SecurityAuditReport {
            summary: "All clear".into(),
            vulnerabilities: vec![],
            passed: true,
        };
        assert!(report.passed);
        assert!(report.vulnerabilities.is_empty());
    }

    #[test]
    fn test_audit_report_with_vulnerabilities() {
        let report = SecurityAuditReport {
            summary: "Found issues".into(),
            vulnerabilities: vec![Vulnerability {
                vuln_type: "XSS".into(),
                severity: SecuritySeverity::High,
                line: Some(10),
                message: "Unescaped user input".into(),
                fix: "Escape output".into(),
                cve_cwe: Some("CWE-79".into()),
            }],
            passed: false,
        };
        assert!(!report.passed);
        assert_eq!(report.vulnerabilities.len(), 1);
    }

    #[test]
    fn test_audit_report_serde_roundtrip() {
        let report = SecurityAuditReport {
            summary: "Security audit complete".into(),
            vulnerabilities: vec![
                Vulnerability {
                    vuln_type: "Race Condition".into(),
                    severity: SecuritySeverity::Medium,
                    line: Some(100),
                    message: "Concurrent access without lock".into(),
                    fix: "Add Mutex".into(),
                    cve_cwe: None,
                },
                Vulnerability {
                    vuln_type: "Hardcoded Secret".into(),
                    severity: SecuritySeverity::Critical,
                    line: Some(200),
                    message: "API key in source".into(),
                    fix: "Use environment variable".into(),
                    cve_cwe: Some("CWE-798".into()),
                },
            ],
            passed: false,
        };
        let json = serde_json::to_string(&report).unwrap();
        let deserialized: SecurityAuditReport = serde_json::from_str(&json).unwrap();
        assert_eq!(
            report.vulnerabilities.len(),
            deserialized.vulnerabilities.len()
        );
        assert_eq!(report.summary, deserialized.summary);
        assert_eq!(report.passed, deserialized.passed);
    }

    #[test]
    fn test_audit_report_serde_default_passed() {
        // Verify that `passed` defaults to false when missing (serde(default))
        let json = r#"{"summary":"test","vulnerabilities":[]}"#;
        let report: SecurityAuditReport = serde_json::from_str(json).unwrap();
        assert!(!report.passed, "passed should default to false");
    }

    // ── AttackVector ──────────────────────────────────────

    #[test]
    fn test_attack_vector_construction() {
        let v = AttackVector {
            vector: "MITM".into(),
            description: "Intercept gRPC traffic".into(),
            impact: "Data leakage".into(),
            mitigation: "Use TLS".into(),
        };
        assert_eq!(v.vector, "MITM");
        assert_eq!(v.mitigation, "Use TLS");
    }

    #[test]
    fn test_attack_vector_serde() {
        let v = AttackVector {
            vector: "DoS".into(),
            description: "Flood with requests".into(),
            impact: "Service unavailable".into(),
            mitigation: "Rate limiting".into(),
        };
        let json = serde_json::to_string(&v).unwrap();
        let deserialized: AttackVector = serde_json::from_str(&json).unwrap();
        assert_eq!(v.vector, deserialized.vector);
    }

    // ── ThreatModelReport ─────────────────────────────────

    #[test]
    fn test_threat_model_report() {
        let report = ThreatModelReport {
            summary: "Architecture has moderate risk".into(),
            attack_vectors: vec![AttackVector {
                vector: "Privilege Escalation".into(),
                description: "Weak RBAC".into(),
                impact: "Unauthorized access".into(),
                mitigation: "Implement ABAC".into(),
            }],
            overall_risk: SecuritySeverity::High,
        };
        assert_eq!(report.attack_vectors.len(), 1);
        assert_eq!(report.overall_risk, SecuritySeverity::High);
    }

    #[test]
    fn test_threat_model_report_serde() {
        let report = ThreatModelReport {
            summary: "Low risk".into(),
            attack_vectors: vec![],
            overall_risk: SecuritySeverity::Low,
        };
        let json = serde_json::to_string(&report).unwrap();
        let deserialized: ThreatModelReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report.summary, deserialized.summary);
        assert_eq!(report.overall_risk, deserialized.overall_risk);
    }

    #[test]
    fn test_threat_model_report_default_attack_vectors() {
        let json = r#"{"summary":"minimal","overall_risk":"Low"}"#;
        let report: ThreatModelReport = serde_json::from_str(json).unwrap();
        assert!(report.attack_vectors.is_empty());
    }
}
