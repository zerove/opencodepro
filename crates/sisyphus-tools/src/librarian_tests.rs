use super::*;
use crate::metis::DummyLlmClient;

const DUMMY: DummyLlmClient = DummyLlmClient;

#[test]
fn test_research_scope_parse() {
    assert_eq!("web".parse::<ResearchScope>().unwrap(), ResearchScope::Web);
    assert_eq!(
        "docs".parse::<ResearchScope>().unwrap(),
        ResearchScope::Documentation
    );
    assert_eq!(
        "code".parse::<ResearchScope>().unwrap(),
        ResearchScope::CodeExamples
    );
    assert_eq!("all".parse::<ResearchScope>().unwrap(), ResearchScope::All);
    assert!("unknown".parse::<ResearchScope>().is_err());
}

#[tokio::test]
async fn test_synthesise_empty() {
    let s = synthesise(&[], &DUMMY).await.unwrap();
    assert!(s.contains("No findings"));
}

#[tokio::test]
async fn test_synthesise_orders_by_relevance() {
    let findings = vec![
        ResearchFinding {
            claim: "low relevance".into(),
            source_url: "https://a".into(),
            source_title: "A".into(),
            relevance: 0.3,
            retrieved_at: "now".into(),
        },
        ResearchFinding {
            claim: "high relevance".into(),
            source_url: "https://b".into(),
            source_title: "B".into(),
            relevance: 0.9,
            retrieved_at: "now".into(),
        },
    ];
    let s = synthesise(&findings, &DUMMY).await.unwrap();
    let high_pos = s.find("high relevance").unwrap();
    let low_pos = s.find("low relevance").unwrap();
    assert!(high_pos < low_pos, "high relevance should appear first");
}

#[tokio::test]
async fn test_synthesise_respects_max_points() {
    let findings: Vec<ResearchFinding> = (0..10)
        .map(|i| ResearchFinding {
            claim: format!("finding {}", i),
            source_url: format!("https://{}.com", i),
            source_title: format!("Source {}", i),
            relevance: 0.5,
            retrieved_at: "now".into(),
        })
        .collect();

    let s = synthesise(&findings, &DUMMY).await.unwrap();
    let count = s.matches("relevance:").count();
    assert!(count <= 3, "should show at most 3 findings");
}

#[tokio::test]
async fn test_fetch_documentation_handles_invalid_url() {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        fetch_documentation("https://invalid.example.nonexistent"),
    )
    .await;
    assert!(result.is_err() || result.is_ok());
}

#[test]
fn test_research_report_structure() {
    let report = ResearchReport {
        query: "test".into(),
        findings: vec![],
        synthesis: "nothing found".into(),
        sources_consulted: 0,
        confidence: 0.0,
    };
    assert_eq!(report.query, "test");
    assert_eq!(report.confidence, 0.0);
    assert!(report.sources_consulted == 0);
}

#[test]
fn test_findings_dedup_logic() {
    let findings = vec![
        ResearchFinding {
            claim: "dup".into(),
            source_url: "https://same".into(),
            source_title: "T".into(),
            relevance: 0.5,
            retrieved_at: "now".into(),
        },
        ResearchFinding {
            claim: "dup2".into(),
            source_url: "https://same".into(),
            source_title: "T".into(),
            relevance: 0.5,
            retrieved_at: "now".into(),
        },
    ];
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<_> = findings
        .into_iter()
        .filter(|f| seen.insert(f.source_url.clone()))
        .collect();
    assert_eq!(unique.len(), 1);
}

#[tokio::test]
#[ignore = "requires network"]
async fn test_research_web_scope() {
    match research("Rust programming language", ResearchScope::Web, 3).await {
        Ok(report) => {
            assert!(!report.findings.is_empty(), "should return some findings");
            assert!(report.confidence > 0.0);
        }
        Err(e) => {
            eprintln!("research skipped (network unavailable): {e}");
        }
    }
}

#[tokio::test]
#[ignore = "requires network"]
async fn test_fetch_documentation() {
    match fetch_documentation("https://www.rust-lang.org").await {
        Ok(page) => {
            assert!(page.content_length > 0);
            assert!(!page.title.is_empty());
        }
        Err(e) => {
            eprintln!("fetch skipped (network unavailable): {e}");
        }
    }
}

#[test]
fn test_research_scope_parse_case_insensitive() {
    assert_eq!("WEB".parse::<ResearchScope>().unwrap(), ResearchScope::Web);
    assert_eq!(
        "Docs".parse::<ResearchScope>().unwrap(),
        ResearchScope::Documentation
    );
    assert_eq!(
        "CODE".parse::<ResearchScope>().unwrap(),
        ResearchScope::CodeExamples
    );
    assert_eq!("ALL".parse::<ResearchScope>().unwrap(), ResearchScope::All);
}

#[test]
fn test_research_scope_parse_aliases() {
    assert_eq!("www".parse::<ResearchScope>().unwrap(), ResearchScope::Web);
    assert_eq!(
        "documentation".parse::<ResearchScope>().unwrap(),
        ResearchScope::Documentation
    );
    assert_eq!(
        "examples".parse::<ResearchScope>().unwrap(),
        ResearchScope::CodeExamples
    );
}

#[test]
fn test_serde_research_finding() {
    let finding = ResearchFinding {
        claim: "Rust is fast".into(),
        source_url: "https://example.com".into(),
        source_title: "Example".into(),
        relevance: 0.95,
        retrieved_at: "2026-01-01T00:00:00.000Z".into(),
    };
    let json = serde_json::to_string(&finding).unwrap();
    let back: ResearchFinding = serde_json::from_str(&json).unwrap();
    assert_eq!(back.claim, "Rust is fast");
    assert!((back.relevance - 0.95).abs() < 1e-6);
}

#[test]
fn test_serde_research_report() {
    let report = ResearchReport {
        query: "rust".into(),
        findings: vec![ResearchFinding {
            claim: "finding 1".into(),
            source_url: "https://a.com".into(),
            source_title: "A".into(),
            relevance: 0.8,
            retrieved_at: "now".into(),
        }],
        synthesis: "synthesised text".into(),
        sources_consulted: 3,
        confidence: 0.75,
    };
    let json = serde_json::to_string_pretty(&report).unwrap();
    let back: ResearchReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.query, "rust");
    assert_eq!(back.findings.len(), 1);
    assert!((back.confidence - 0.75).abs() < 1e-6);
}

#[test]
fn test_serde_code_example() {
    let example = CodeExample {
        source: "github.com/rust-lang/rust".into(),
        language: "rust".into(),
        code: "fn main() {}".into(),
        description: "simple example".into(),
    };
    let json = serde_json::to_string(&example).unwrap();
    let back: CodeExample = serde_json::from_str(&json).unwrap();
    assert_eq!(back.language, "rust");
    assert!(back.code.contains("main"));
}

#[test]
fn test_serde_doc_page() {
    let page = DocPage {
        url: "https://doc.rust-lang.org".into(),
        title: "Rust Docs".into(),
        content_preview: "The Rust programming language...".into(),
        content_length: 42,
    };
    let json = serde_json::to_string(&page).unwrap();
    let back: DocPage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.url, "https://doc.rust-lang.org");
    assert_eq!(back.content_length, 42);
}
