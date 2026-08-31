//! Code review — heuristic checks + LLM semantic review for logic issues,
//! performance, security, and maintainability.

use super::types::{LlmReviewResponse, MomusReview, ReviewComment, ReviewSeverity, ReviewType};
use sisyphus_tools_core::ToolResult;
use sisyphus_tools_llm::metis::LlmClient;
use std::path::Path;

const CODE_REVIEW_SYSTEM_PROMPT: &str = r#"You are Momus, a senior engineer and quality gatekeeper.
Your goal is to perform a deep semantic review of source code.

Analyze the code and return a JSON object with the following structure:
{
  "passed": true/false,
  "summary": "Concise summary of the review",
  "comments": [
    {
      "severity": "Blocker|Critical|Major|Minor|Nitpick",
      "category": "logic|performance|concurrency|maintainability|security|style",
      "file": "path/to/file",
      "line": 123,
      "message": "Detailed explanation of the issue",
      "suggestion": "How to fix it"
    }
  ]
}

Guidelines:
1. Logic: Look for edge cases, off-by-one errors, incorrect state transitions, and potential bugs.
2. Performance: Identify inefficient algorithms, unnecessary allocations, or blocking calls in async contexts.
3. Concurrency (Rust specific): Check for deadlocks, race conditions, improper use of Arc/Mutex/RwLock, and async safety.
4. Maintainability: Evaluate readability, modularity, and adherence to SRP (Single Responsibility Principle).
5. Security: Look for common vulnerabilities (e.g., injection, overflow, improper validation).
6. Severity:
   - Blocker: Critical bugs, security holes, or deadlocks.
   - Critical: Major logic errors or severe performance issues.
   - Major: Significant maintainability or correctness issues.
   - Minor: Small improvements or potential edge cases.
   - Nitpick: Style, naming, or minor optimizations.
"#;

/// Review source code for common issues.
pub async fn review_code(
    client: &dyn LlmClient,
    path: &Path,
    content: &str,
) -> ToolResult<MomusReview> {
    let mut comments = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // 1. Heuristic checks (Fast feedback)
    for (i, line) in lines.iter().enumerate() {
        if line.len() > 120 {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Nitpick,
                category: "style".into(),
                file: Some(path.to_string_lossy().to_string()),
                line: Some(i + 1),
                message: format!("Line too long ({} chars, max 120)", line.len()),
                suggestion: Some(
                    "Break into multiple lines or extract an intermediate variable.".into(),
                ),
            });
        }
    }

    for (i, line) in lines.iter().enumerate() {
        let upper = line.to_uppercase();
        if upper.contains("TODO")
            || upper.contains("FIXME")
            || upper.contains("HACK")
            || upper.contains("XXX")
        {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Minor,
                category: "completeness".into(),
                file: Some(path.to_string_lossy().to_string()),
                line: Some(i + 1),
                message: format!("Contains marker: {}", line.trim()),
                suggestion: Some(
                    "Resolve before considering this complete, or file a tracking issue.".into(),
                ),
            });
        }
    }

    let mut in_fn = false;
    let mut fn_start = 0;
    let mut brace_depth = 0;
    let mut fn_name = String::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if in_fn {
            brace_depth += trimmed.chars().filter(|&c| c == '{').count();
            brace_depth -= trimmed.chars().filter(|&c| c == '}').count();
            if brace_depth == 0 {
                in_fn = false;
                let fn_lines = i - fn_start + 1;
                if fn_lines > 50 {
                    comments.push(ReviewComment {
                        severity: ReviewSeverity::Major,
                        category: "complexity".into(),
                        file: Some(path.to_string_lossy().to_string()),
                        line: Some(fn_start + 1),
                        message: format!(
                            "Function '{fn_name}' is {fn_lines} lines long (recommended max: 50)."
                        ),
                        suggestion: Some(
                            "Consider extracting helper functions to reduce complexity.".into(),
                        ),
                    });
                }
            }
        } else {
            if let Some(name) = super::extract_fn_name(trimmed) {
                in_fn = true;
                fn_start = i;
                fn_name = name;
                brace_depth = trimmed.chars().filter(|&c| c == '{').count();
                if brace_depth > 0 && !trimmed.ends_with('{') && !trimmed.contains(';') {
                    brace_depth = 0;
                    in_fn = false;
                }
            }
        }
    }

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub fn")
            || trimmed.starts_with("pub struct")
            || trimmed.starts_with("pub enum")
            || trimmed.starts_with("pub trait")
        {
            let has_doc = i > 0
                && (lines[i - 1].trim().starts_with("///")
                    || lines[i - 1].trim().starts_with("//!"));
            if !has_doc {
                comments.push(ReviewComment {
                    severity: ReviewSeverity::Minor,
                    category: "documentation".into(),
                    file: Some(path.to_string_lossy().to_string()),
                    line: Some(i + 1),
                    message: format!("Public item '{}' is undocumented.", trimmed.split_whitespace().nth(1).unwrap_or("?")),
                    suggestion: Some("Add a doc comment (///) explaining what this does, why, and how to use it.".into()),
                });
            }
        }
    }

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if (trimmed.contains(".unwrap()") || trimmed.contains(".expect("))
            && !trimmed.contains("cfg(test)")
        {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Major,
                category: "security".into(),
                file: Some(path.to_string_lossy().to_string()),
                line: Some(i + 1),
                message: "Found .unwrap() or .expect() in potentially production code.".into(),
                suggestion: Some(
                    "Use proper error handling (Result/Option) and propagate errors using '?'."
                        .into(),
                ),
            });
        }
    }

    for (i, line) in lines.iter().enumerate() {
        let indent = line.len() - line.trim_start().len();
        if indent > 80 {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Major,
                category: "complexity".into(),
                file: Some(path.to_string_lossy().to_string()),
                line: Some(i + 1),
                message: "Deeply nested code (indentation > 80 chars).".into(),
                suggestion: Some(
                    "Extract inner logic into a helper function or use early returns.".into(),
                ),
            });
        }
    }

    // 2. Semantic review (LLM)
    let user_prompt = format!("File: {}\n\nContent:\n{}", path.to_string_lossy(), content);
    let llm_result = client.chat(CODE_REVIEW_SYSTEM_PROMPT, &user_prompt, true).await;

    let (llm_passed, llm_summary, llm_comments) = match llm_result {
        Ok(response) => match serde_json::from_str::<LlmReviewResponse>(&response) {
            Ok(res) => (
                res.passed,
                res.summary,
                res.comments.into_iter().map(ReviewComment::from).collect(),
            ),
            Err(e) => {
                comments.push(ReviewComment {
                    severity: ReviewSeverity::Minor,
                    category: "system".into(),
                    file: Some(path.to_string_lossy().to_string()),
                    line: None,
                    message: format!("Semantic review failed to parse: {e}"),
                    suggestion: None,
                });
                (
                    true,
                    "Semantic review skipped due to parsing error.".to_string(),
                    vec![],
                )
            },
        },
        Err(e) => {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Minor,
                category: "system".into(),
                file: Some(path.to_string_lossy().to_string()),
                line: None,
                message: format!("Semantic review failed: {e}"),
                suggestion: None,
            });
            (
                true,
                "Semantic review skipped due to LLM error.".to_string(),
                vec![],
            )
        },
    };

    comments.extend(llm_comments);

    let passed = comments
        .iter()
        .all(|c| c.severity != ReviewSeverity::Blocker && c.severity != ReviewSeverity::Critical)
        && llm_passed;

    let summary = if passed {
        format!(
            "✅ Code review PASSED ({} issues found). Summary: {}",
            comments.len(),
            llm_summary
        )
    } else {
        format!(
            "❌ Code review FAILED ({} issues found). Summary: {}",
            comments.len(),
            llm_summary
        )
    };

    Ok(MomusReview {
        target: path.to_string_lossy().to_string(),
        review_type: ReviewType::CodeReview,
        passed,
        comments,
        summary,
    })
}

/// Run code review on a batch of files.
pub async fn review_code_batch(
    client: &dyn LlmClient,
    files: &[(&Path, &str)],
) -> ToolResult<Vec<MomusReview>> {
    let mut reviews = Vec::new();
    for (path, content) in files {
        reviews.push(review_code(client, path, content).await?);
    }
    Ok(reviews)
}
