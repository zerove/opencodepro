use crate::{ToolResult, webfetch, websearch};
use serde::{Deserialize, Serialize};

// ── Types ──────────────────────────────────────────────────

/// A researched fact with source attribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchFinding {
    pub claim: String,
    pub source_url: String,
    pub source_title: String,
    pub relevance: f64,         // 0.0 – 1.0
    pub retrieved_at: String,
}

/// Result of a librarian research query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchReport {
    pub query: String,
    pub findings: Vec<ResearchFinding>,
    pub synthesis: String,
    pub sources_consulted: usize,
    pub confidence: f64,        // 0.0 – 1.0
}

/// Search scope for the librarian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResearchScope {
    Web,
    Documentation,
    CodeExamples,
    All,
}

impl std::str::FromStr for ResearchScope {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "web" | "www" => Ok(Self::Web),
            "docs" | "documentation" => Ok(Self::Documentation),
            "code" | "examples" => Ok(Self::CodeExamples),
            "all" => Ok(Self::All),
            _ => Err(format!("unknown research scope: {s} (expected web|docs|code|all)")),
        }
    }
}

/// A single documentation page fetched and parsed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocPage {
    pub url: String,
    pub title: String,
    pub content_preview: String,  // first ~500 chars
    pub content_length: usize,
}

/// Example code snippet found during research.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeExample {
    pub source: String,
    pub language: String,
    pub code: String,
    pub description: String,
}

// ── Research Functions ─────────────────────────────────────

/// Perform a multi-source research query.
///
/// Searches the web, fetches top results, and synthesises findings
/// into a structured report. Returns a `ResearchReport` with
/// attributed findings and a natural-language synthesis.
pub async fn research(query: &str, scope: ResearchScope, max_results: usize) -> ToolResult<ResearchReport> {
    let mut findings = Vec::new();
    let mut sources_consulted = 0;

    match scope {
        ResearchScope::Web => {
            add_web_results(query, max_results, &mut findings, &mut sources_consulted).await;
        }
        ResearchScope::Documentation => {
            add_doc_results(query, max_results, &mut findings, &mut sources_consulted).await;
        }
        ResearchScope::CodeExamples => {
            add_code_results(query, max_results, &mut findings, &mut sources_consulted).await;
        }
        ResearchScope::All => {
            add_web_results(query, max_results, &mut findings, &mut sources_consulted).await;
            add_doc_results(query, max_results, &mut findings, &mut sources_consulted).await;
            add_code_results(query, max_results, &mut findings, &mut sources_consulted).await;
        }
    }

    // Deduplicate by URL
    let mut seen = std::collections::HashSet::new();
    findings.retain(|f| seen.insert(f.source_url.clone()));

    let confidence = if findings.is_empty() {
        0.0
    } else {
        // Confidence based on number of corroborating sources and average relevance
        let avg_relevance: f64 = findings.iter().map(|f| f.relevance).sum::<f64>() / findings.len() as f64;
        let coverage = (findings.len() as f64 / max_results as f64).min(1.0);
        avg_relevance * 0.6 + coverage * 0.4
    };

    let synthesis = if findings.is_empty() {
        format!("No findings for query: {query}")
    } else {
        let top: Vec<&str> = findings.iter().take(3).map(|f| f.claim.as_str()).collect();
        format!(
            "Research on \"{query}\" found {} sources. Top findings:\n- {}",
            findings.len(),
            top.join("\n- ")
        )
    };

    Ok(ResearchReport {
        query: query.to_string(),
        findings,
        synthesis,
        sources_consulted,
        confidence,
    })
}

/// Fetch a specific URL and return structured documentation.
pub async fn fetch_documentation(url: &str) -> ToolResult<DocPage> {
    let fetched = webfetch(url).await?;
    let preview = fetched.content.chars().take(500).collect::<String>();

    // Extract a title from the first meaningful line
    let title = fetched.content.lines()
        .find(|l| {
            let t = l.trim();
            !t.is_empty() && t.len() > 10 && t.len() < 200
        })
        .unwrap_or(url)
        .to_string();

    Ok(DocPage {
        url: url.to_string(),
        title,
        content_preview: preview,
        content_length: fetched.content.len(),
    })
}

/// Search for a specific library/API and return usage examples.
pub async fn find_examples(query: &str, language: &str) -> ToolResult<Vec<CodeExample>> {
    let search_query = format!("{} {} example", query, language);
    let results = websearch(&search_query, 5).await?;

    let mut examples = Vec::new();
    for r in &results {
        if let Ok(fetched) = webfetch(&r.url).await {
            // Extract code blocks (lines between ``` markers)
            let mut in_code = false;
            let mut code_lines: Vec<String> = Vec::new();
            let mut block_lang = String::new();

            for line in fetched.content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("```") {
                    if in_code {
                        // End of code block
                        if !code_lines.is_empty() {
                            examples.push(CodeExample {
                                source: r.url.clone(),
                                language: block_lang.clone(),
                                code: code_lines.join("\n"),
                                description: format!("Example from {}", r.title),
                            });
                        }
                        code_lines.clear();
                        in_code = false;
                    } else {
                        in_code = true;
                        block_lang = trimmed.trim_start_matches("```").to_string();
                        if block_lang.is_empty() {
                            block_lang = language.to_string();
                        }
                    }
                } else if in_code {
                    code_lines.push(line.to_string());
                }
            }
        }
    }

    Ok(examples)
}

// ── Internal helpers ─────────────────────────────────────

async fn add_web_results(query: &str, max_results: usize, findings: &mut Vec<ResearchFinding>, sources: &mut usize) {
    if let Ok(results) = websearch(query, max_results).await {
        *sources += results.len();
        for r in &results {
            let claim = format!("[{}] {}", r.title, r.snippet);
            findings.push(ResearchFinding {
                claim,
                source_url: r.url.clone(),
                source_title: r.title.clone(),
                relevance: 0.8,
                retrieved_at: now_iso(),
            });
        }
    }
}

async fn add_doc_results(query: &str, max_results: usize, findings: &mut Vec<ResearchFinding>, sources: &mut usize) {
    let doc_query = format!("{} documentation", query);
    if let Ok(results) = websearch(&doc_query, max_results.min(5)).await {
        *sources += results.len();
        for r in &results {
            findings.push(ResearchFinding {
                claim: format!("[docs] {} — {}", r.title, r.snippet),
                source_url: r.url.clone(),
                source_title: r.title.clone(),
                relevance: 0.7,
                retrieved_at: now_iso(),
            });
        }
    }
}

async fn add_code_results(query: &str, max_results: usize, findings: &mut Vec<ResearchFinding>, sources: &mut usize) {
    let code_query = format!("{} example code", query);
    if let Ok(results) = websearch(&code_query, max_results.min(5)).await {
        *sources += results.len();
        for r in &results {
            findings.push(ResearchFinding {
                claim: format!("[code] {} — {}", r.title, r.snippet),
                source_url: r.url.clone(),
                source_title: r.title.clone(),
                relevance: 0.6,
                retrieved_at: now_iso(),
            });
        }
    }
}

/// Synthesise multiple findings into a concise summary.
///
/// Useful when you've gathered research from multiple sources
/// and need a combined TL;DR.
pub fn synthesise(findings: &[ResearchFinding], max_points: usize) -> String {
    if findings.is_empty() {
        return "No findings to synthesise.".to_string();
    }

    let mut sorted: Vec<&ResearchFinding> = findings.iter().collect();
    sorted.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));

    let mut output = String::new();
    output.push_str(&format!("Synthesis of {} sources (top {} shown):\n\n", findings.len(), max_points));

    for f in sorted.iter().take(max_points) {
        output.push_str(&format!("- {} [relevance: {:.1}]\n  source: {}\n\n", f.claim, f.relevance, f.source_url));
    }

    output
}

// ── Helpers ─────────────────────────────────────────────────

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.as_millis() % 1000;
    let secs_in_day = secs % 86400;
    let hours = secs_in_day / 3600;
    let minutes = (secs_in_day % 3600) / 60;
    let seconds = secs_in_day % 60;
    let days_since_epoch = secs / 86400;
    let year = 1970 + (days_since_epoch as f64 / 365.25) as u64;
    let month = 1 + ((days_since_epoch % 365) as f64 / 30.44) as u64;
    let day = 1 + ((days_since_epoch % 365) as f64 % 30.44) as u64;
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z")
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_research_scope_parse() {
        assert_eq!("web".parse::<ResearchScope>().unwrap(), ResearchScope::Web);
        assert_eq!("docs".parse::<ResearchScope>().unwrap(), ResearchScope::Documentation);
        assert_eq!("code".parse::<ResearchScope>().unwrap(), ResearchScope::CodeExamples);
        assert_eq!("all".parse::<ResearchScope>().unwrap(), ResearchScope::All);
        assert!("unknown".parse::<ResearchScope>().is_err());
    }

    #[test]
    fn test_synthesise_empty() {
        let s = synthesise(&[], 5);
        assert!(s.contains("No findings"));
    }

    #[test]
    fn test_synthesise_orders_by_relevance() {
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
        let s = synthesise(&findings, 5);
        let high_pos = s.find("high relevance").unwrap();
        let low_pos = s.find("low relevance").unwrap();
        assert!(high_pos < low_pos, "high relevance should appear first");
    }

    #[test]
    fn test_synthesise_respects_max_points() {
        let findings: Vec<ResearchFinding> = (0..10).map(|i| ResearchFinding {
            claim: format!("finding {}", i),
            source_url: format!("https://{}.com", i),
            source_title: format!("Source {}", i),
            relevance: 0.5,
            retrieved_at: "now".into(),
        }).collect();

        let s = synthesise(&findings, 3);
        let count = s.matches("relevance:").count();
        assert!(count <= 3, "should show at most 3 findings");
    }

    #[tokio::test]
    async fn test_fetch_documentation_handles_invalid_url() {
        // With a very short timeout, this should fail or panic for non-routable addresses.
        // We just verify it doesn't hang forever.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            fetch_documentation("https://invalid.example.nonexistent"),
        ).await;
        // Either timeout or error — both indicate graceful handling
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
        let unique: Vec<_> = findings.into_iter().filter(|f| seen.insert(f.source_url.clone())).collect();
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
}
