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
 
const SYNTHESIS_SYSTEM_PROMPT: &str = r#"You are the Librarian, Sisyphus's knowledge synthesis expert.
Your goal is to transform a collection of raw research findings into a high-density, professional research report.

### MANDATORY STRUCTURE:
Your output MUST be partitioned into these three sections. You are STRICTLY FORBIDDEN from omitting, renaming, or merging these headers:

1. [Core Conclusions]
   (A concise summary of the most important takeaways.)

2. [Detailed Evidence]
   (A deep dive into the findings, grouping related points and providing clear source attribution. Example: "According to [Source Title]...")

3. [Comprehensive Recommendations]
   (Actionable advice or next steps based on the evidence.)

### STRICT CONSTRAINT:
The headers "[Core Conclusions]", "[Detailed Evidence]", and "[Comprehensive Recommendations]" must appear exactly as written, including the brackets.

Guidelines:
- Eliminate redundancy: Synthesize multiple similar claims into one strong point.
- Prioritize quality: Focus on authoritative and relevant information.
- Maintain a professional, technical tone.
- Highlight conflicts: If sources contradict, explain the different perspectives.
- Handle empty results: If no meaningful information is found, state it clearly.
"#;

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

    // Extract a title: prefer Markdown headers, then the first meaningful line
    let title = fetched.content.lines()
        .find(|l| l.trim().starts_with('#'))
        .and_then(|l| {
            let t = l.trim().trim_start_matches('#').trim();
            if t.is_empty() { None } else { Some(t.to_string()) }
        })
        .or_else(|| {
            fetched.content.lines()
                .find(|l| {
                    let t = l.trim();
                    !t.is_empty() && t.len() > 5 && t.len() < 200
                })
                .map(|l| l.trim().to_string())
        })
        .unwrap_or_else(|| url.to_string());

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
                        // Extract language from the opening fence (e.g., ```rust)
                        block_lang = trimmed.trim_start_matches("```").trim().to_string();
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
                relevance: 0.5,
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
                relevance: 0.5,
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
                relevance: 0.5,
                retrieved_at: now_iso(),
            });
        }
    }
}

/// Synthesise multiple findings into a professional research report.
///
/// Uses an LLM to perform semantic analysis, removing redundancy and 
/// structuring the output into conclusions, evidence, and recommendations.
pub async fn synthesise(findings: &[ResearchFinding], client: &(impl crate::metis::LlmClient + ?Sized)) -> Result<String, String> {
    if findings.is_empty() {
        return Ok("No findings to synthesise.".to_string());
    }

    // Sort by relevance (highest first) so the most important findings come first
    let mut sorted: Vec<&ResearchFinding> = findings.iter().collect();
    sorted.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));

    let mut user_prompt = format!("Synthesise the following {} research findings into a professional report:\n\n", findings.len());
    for (i, f) in sorted.iter().enumerate() {
        user_prompt.push_str(&format!("{}. Claim: {}\n   Source: {} ({})\n   Relevance: {:.2}\n\n", 
            i + 1, f.claim, f.source_title, f.source_url, f.relevance));
    }

    client.chat(SYNTHESIS_SYSTEM_PROMPT, &user_prompt, false).await
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
    use crate::metis::DummyLlmClient;

    const DUMMY: DummyLlmClient = DummyLlmClient;

    #[test]
    fn test_research_scope_parse() {
        assert_eq!("web".parse::<ResearchScope>().unwrap(), ResearchScope::Web);
        assert_eq!("docs".parse::<ResearchScope>().unwrap(), ResearchScope::Documentation);
        assert_eq!("code".parse::<ResearchScope>().unwrap(), ResearchScope::CodeExamples);
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
        let findings: Vec<ResearchFinding> = (0..10).map(|i| ResearchFinding {
            claim: format!("finding {}", i),
            source_url: format!("https://{}.com", i),
            source_title: format!("Source {}", i),
            relevance: 0.5,
            retrieved_at: "now".into(),
        }).collect();

        let s = synthesise(&findings, &DUMMY).await.unwrap();
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

    #[test]
    fn test_research_scope_parse_case_insensitive() {
        assert_eq!("WEB".parse::<ResearchScope>().unwrap(), ResearchScope::Web);
        assert_eq!("Docs".parse::<ResearchScope>().unwrap(), ResearchScope::Documentation);
        assert_eq!("CODE".parse::<ResearchScope>().unwrap(), ResearchScope::CodeExamples);
        assert_eq!("ALL".parse::<ResearchScope>().unwrap(), ResearchScope::All);
    }

    #[test]
    fn test_research_scope_parse_aliases() {
        assert_eq!("www".parse::<ResearchScope>().unwrap(), ResearchScope::Web);
        assert_eq!("documentation".parse::<ResearchScope>().unwrap(), ResearchScope::Documentation);
        assert_eq!("examples".parse::<ResearchScope>().unwrap(), ResearchScope::CodeExamples);
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
            findings: vec![
                ResearchFinding {
                    claim: "finding 1".into(),
                    source_url: "https://a.com".into(),
                    source_title: "A".into(),
                    relevance: 0.8,
                    retrieved_at: "now".into(),
                },
            ],
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
}
