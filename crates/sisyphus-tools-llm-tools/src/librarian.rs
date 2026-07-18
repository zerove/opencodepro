//! Librarian: multi-source research engine.
//!
//! Performs web searches, fetches documentation, finds code examples,
//! and synthesises findings into structured research reports.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (ResearchFinding, ResearchReport, ResearchScope, DocPage, CodeExample).

pub mod types;

pub use types::{now_iso, CodeExample, DocPage, ResearchFinding, ResearchReport, ResearchScope};

use sisyphus_tools_core::ToolResult;
use sisyphus_tools_system::web::{webfetch, websearch};

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
pub async fn research(
    query: &str,
    scope: ResearchScope,
    max_results: usize,
) -> ToolResult<ResearchReport> {
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
        let avg_relevance: f64 =
            findings.iter().map(|f| f.relevance).sum::<f64>() / findings.len() as f64;
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
    let title = fetched
        .content
        .lines()
        .find(|l| l.trim().starts_with('#'))
        .and_then(|l| {
            let t = l.trim().trim_start_matches('#').trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
        .or_else(|| {
            fetched
                .content
                .lines()
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
    let search_query = format!("{query} {language} example");
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

async fn add_web_results(
    query: &str,
    max_results: usize,
    findings: &mut Vec<ResearchFinding>,
    sources: &mut usize,
) {
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

async fn add_doc_results(
    query: &str,
    max_results: usize,
    findings: &mut Vec<ResearchFinding>,
    sources: &mut usize,
) {
    let doc_query = format!("{query} documentation");
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

async fn add_code_results(
    query: &str,
    max_results: usize,
    findings: &mut Vec<ResearchFinding>,
    sources: &mut usize,
) {
    let code_query = format!("{query} example code");
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
pub async fn synthesise(
    findings: &[ResearchFinding],
    client: &(impl sisyphus_tools_llm::metis::LlmClient + ?Sized),
) -> Result<String, String> {
    if findings.is_empty() {
        return Ok("No findings to synthesise.".to_string());
    }

    // Sort by relevance (highest first) so the most important findings come first
    let mut sorted: Vec<&ResearchFinding> = findings.iter().collect();
    sorted.sort_by(|a, b| {
        b.relevance
            .partial_cmp(&a.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut user_prompt = format!(
        "Synthesise the following {} research findings into a professional report:\n\n",
        findings.len()
    );
    for (i, f) in sorted.iter().enumerate() {
        user_prompt.push_str(&format!(
            "{}. Claim: {}\n   Source: {} ({})\n   Relevance: {:.2}\n\n",
            i + 1,
            f.claim,
            f.source_title,
            f.source_url,
            f.relevance
        ));
    }

    client
        .chat(SYNTHESIS_SYSTEM_PROMPT, &user_prompt, false)
        .await
}

// ── Tests ───────────────────────────────────────────────────
