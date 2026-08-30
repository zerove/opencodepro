//! 集成测试：tools-llm-tools security（安全审计类型）
//!
//! 覆盖 `sisyphus_tools_llm_tools::security` 的纯数据模块：
//! - `SecuritySeverity` 4 等级 + Display + serde
//! - `Vulnerability` 字段
//! - `SecurityAuditReport` 默认 passed 与 vulnerabilities
//! - `AttackVector` 字段
//! - `ThreatModelReport` 字段

use sisyphus_tools_llm_tools::security::{
    AttackVector, SecurityAuditReport, SecuritySeverity, ThreatModelReport, Vulnerability,
};

// ---------------------------------------------------------------------------
// SecuritySeverity
// ---------------------------------------------------------------------------

#[test]
fn severity_serde_round_trip_all() {
    for s in [
        SecuritySeverity::Critical,
        SecuritySeverity::High,
        SecuritySeverity::Medium,
        SecuritySeverity::Low,
    ] {
        let json = serde_json::to_string(&s).unwrap();
        let back: SecuritySeverity = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}

#[test]
fn severity_specific_serde_names() {
    assert_eq!(
        serde_json::to_string(&SecuritySeverity::Critical).unwrap(),
        "\"Critical\""
    );
    assert_eq!(
        serde_json::to_string(&SecuritySeverity::High).unwrap(),
        "\"High\""
    );
    assert_eq!(
        serde_json::to_string(&SecuritySeverity::Medium).unwrap(),
        "\"Medium\""
    );
    assert_eq!(
        serde_json::to_string(&SecuritySeverity::Low).unwrap(),
        "\"Low\""
    );
}

#[test]
fn severity_display() {
    assert_eq!(format!("{}", SecuritySeverity::Critical), "Critical");
    assert_eq!(format!("{}", SecuritySeverity::High), "High");
    assert_eq!(format!("{}", SecuritySeverity::Medium), "Medium");
    assert_eq!(format!("{}", SecuritySeverity::Low), "Low");
}

#[test]
fn severity_eq_clone() {
    // No Copy derive; verify Clone works.
    let a = SecuritySeverity::High;
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn severity_ordering_consistent() {
    assert_ne!(SecuritySeverity::Critical, SecuritySeverity::Low);
    assert_eq!(SecuritySeverity::Medium, SecuritySeverity::Medium);
}

// ---------------------------------------------------------------------------
// Vulnerability
// ---------------------------------------------------------------------------

fn make_vuln(severity: SecuritySeverity) -> Vulnerability {
    Vulnerability {
        vuln_type: "SQL Injection".to_string(),
        severity,
        line: Some(42),
        message: "Unparameterized query".to_string(),
        fix: "Use prepared statements".to_string(),
        cve_cwe: Some("CWE-89".to_string()),
    }
}

#[test]
fn vuln_field_access() {
    let v = make_vuln(SecuritySeverity::Critical);
    assert_eq!(v.vuln_type, "SQL Injection");
    assert_eq!(v.severity, SecuritySeverity::Critical);
    assert_eq!(v.line, Some(42));
    assert_eq!(v.message, "Unparameterized query");
    assert_eq!(v.fix, "Use prepared statements");
    assert_eq!(v.cve_cwe, Some("CWE-89".to_string()));
}

#[test]
fn vuln_with_no_line_no_cve() {
    let v = Vulnerability {
        vuln_type: "X".to_string(),
        severity: SecuritySeverity::Low,
        line: None,
        message: "m".to_string(),
        fix: "f".to_string(),
        cve_cwe: None,
    };
    assert!(v.line.is_none());
    assert!(v.cve_cwe.is_none());
}

#[test]
fn vuln_serde_round_trip() {
    let v = make_vuln(SecuritySeverity::High);
    let json = serde_json::to_string(&v).unwrap();
    let back: Vulnerability = serde_json::from_str(&json).unwrap();
    assert_eq!(back.vuln_type, v.vuln_type);
    assert_eq!(back.severity, v.severity);
    assert_eq!(back.line, v.line);
    assert_eq!(back.cve_cwe, v.cve_cwe);
}

#[test]
fn vuln_clone() {
    let v = make_vuln(SecuritySeverity::High);
    let v2 = v.clone();
    assert_eq!(v.vuln_type, v2.vuln_type);
    assert_eq!(v.severity, v2.severity);
}

// ---------------------------------------------------------------------------
// SecurityAuditReport
// ---------------------------------------------------------------------------

#[test]
fn audit_report_empty() {
    let r = SecurityAuditReport {
        summary: "ok".to_string(),
        vulnerabilities: vec![],
        passed: true,
    };
    assert!(r.vulnerabilities.is_empty());
    assert!(r.passed);
    assert_eq!(r.summary, "ok");
}

#[test]
fn audit_report_with_vulns() {
    let r = SecurityAuditReport {
        summary: "issues found".to_string(),
        vulnerabilities: vec![
            make_vuln(SecuritySeverity::Critical),
            make_vuln(SecuritySeverity::Low),
        ],
        passed: false,
    };
    assert_eq!(r.vulnerabilities.len(), 2);
    assert!(!r.passed);
}

#[test]
fn audit_report_serde_default_passes_false() {
    // #[serde(default)] on bool uses Default::default() = false
    let json = r#"{"summary":"x","vulnerabilities":[]}"#;
    let r: SecurityAuditReport = serde_json::from_str(json).unwrap();
    assert!(!r.passed);
}

#[test]
fn audit_report_serde_default_vulns_empty() {
    // Verify #[serde(default)] on vulnerabilities produces empty when missing
    let json = r#"{"summary":"x","passed":true}"#;
    let r: SecurityAuditReport = serde_json::from_str(json).unwrap();
    assert!(r.vulnerabilities.is_empty());
}

#[test]
fn audit_report_serde_full() {
    let r = SecurityAuditReport {
        summary: "ok".to_string(),
        vulnerabilities: vec![make_vuln(SecuritySeverity::High)],
        passed: false,
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: SecurityAuditReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.summary, "ok");
    assert_eq!(back.passed, false);
    assert_eq!(back.vulnerabilities.len(), 1);
}

// ---------------------------------------------------------------------------
// AttackVector
// ---------------------------------------------------------------------------

#[test]
fn attack_vector_field_access() {
    let av = AttackVector {
        vector: "HTTP".to_string(),
        description: "unencrypted".to_string(),
        impact: "data leak".to_string(),
        mitigation: "use TLS".to_string(),
    };
    assert_eq!(av.vector, "HTTP");
    assert_eq!(av.description, "unencrypted");
    assert_eq!(av.impact, "data leak");
    assert_eq!(av.mitigation, "use TLS");
}

#[test]
fn attack_vector_serde_round_trip() {
    let av = AttackVector {
        vector: "V".to_string(),
        description: "D".to_string(),
        impact: "I".to_string(),
        mitigation: "M".to_string(),
    };
    let json = serde_json::to_string(&av).unwrap();
    let back: AttackVector = serde_json::from_str(&json).unwrap();
    assert_eq!(back.vector, av.vector);
    assert_eq!(back.description, av.description);
    assert_eq!(back.impact, av.impact);
    assert_eq!(back.mitigation, av.mitigation);
}

#[test]
fn attack_vector_clone() {
    let av = AttackVector {
        vector: "V".to_string(),
        description: "D".to_string(),
        impact: "I".to_string(),
        mitigation: "M".to_string(),
    };
    let av2 = av.clone();
    assert_eq!(av.vector, av2.vector);
}

// ---------------------------------------------------------------------------
// ThreatModelReport
// ---------------------------------------------------------------------------

#[test]
fn threat_model_empty() {
    let r = ThreatModelReport {
        summary: "low risk".to_string(),
        attack_vectors: vec![],
        overall_risk: SecuritySeverity::Low,
    };
    assert!(r.attack_vectors.is_empty());
    assert_eq!(r.overall_risk, SecuritySeverity::Low);
}

#[test]
fn threat_model_with_vectors() {
    let r = ThreatModelReport {
        summary: "high".to_string(),
        attack_vectors: vec![
            AttackVector {
                vector: "A".to_string(),
                description: "D".to_string(),
                impact: "I".to_string(),
                mitigation: "M".to_string(),
            },
            AttackVector {
                vector: "B".to_string(),
                description: "D2".to_string(),
                impact: "I2".to_string(),
                mitigation: "M2".to_string(),
            },
        ],
        overall_risk: SecuritySeverity::Critical,
    };
    assert_eq!(r.attack_vectors.len(), 2);
    assert_eq!(r.overall_risk, SecuritySeverity::Critical);
}

#[test]
fn threat_model_serde_default_attack_vectors_empty() {
    let json = r#"{"summary":"s","overall_risk":"High"}"#;
    let r: ThreatModelReport = serde_json::from_str(json).unwrap();
    assert!(r.attack_vectors.is_empty());
    assert_eq!(r.overall_risk, SecuritySeverity::High);
}

#[test]
fn threat_model_serde_full() {
    let r = ThreatModelReport {
        summary: "x".to_string(),
        attack_vectors: vec![AttackVector {
            vector: "V".to_string(),
            description: "D".to_string(),
            impact: "I".to_string(),
            mitigation: "M".to_string(),
        }],
        overall_risk: SecuritySeverity::High,
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: ThreatModelReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.summary, "x");
    assert_eq!(back.attack_vectors.len(), 1);
    assert_eq!(back.overall_risk, SecuritySeverity::High);
}
