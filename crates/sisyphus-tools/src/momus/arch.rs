//! Architecture review — heuristic checks + LLM semantic review for
//! consistency, modularity, and extensibility.

use super::types::{LlmReviewResponse, MomusReview, ReviewComment, ReviewSeverity, ReviewType};
use crate::metis::LlmClient;

const ARCH_REVIEW_SYSTEM_PROMPT: &str = r#"You are Momus, a senior architect and quality gatekeeper.
Your goal is to review architecture documentation for consistency, modularity, and extensibility.

Analyze the architecture text and return a JSON object with the following structure:
{
  "passed": true/false,
  "summary": "Concise summary of the review",
  "comments": [
    {
      "severity": "Blocker|Critical|Major|Minor|Nitpick",
      "category": "completeness|decoupling|srp|extensibility|consistency",
      "file": null,
      "line": null,
      "message": "Detailed explanation of the issue",
      "suggestion": "How to fix it"
    }
  ]
}

Guidelines:
1. Completeness: Are all major components described? Are interactions and data flows clear?
2. Decoupling: Are components too tightly coupled? Are there circular dependencies?
3. SRP (Single Responsibility Principle): Does each component have a clear, single purpose?
4. Extensibility: Is the design flexible enough to accommodate future changes?
5. Consistency: Is the terminology consistent throughout the document?
6. Severity:
   - Blocker: Fundamental architectural flaws or missing core components.
   - Critical: Major coupling issues or SRP violations.
   - Major: Significant gaps in documentation or design.
   - Minor: Small inconsistencies or suggestions for better decoupling.
   - Nitpick: Wording or formatting suggestions.
"#;

/// Review architecture documentation for consistency.
pub async fn review_architecture(client: &dyn LlmClient, arch_text: &str) -> MomusReview {
    let mut comments = Vec::new();

    // 1. Heuristic checks (Fast feedback)
    if arch_text.trim().is_empty() {
        comments.push(ReviewComment {
            severity: ReviewSeverity::Blocker,
            category: "completeness".into(),
            file: None,
            line: None,
            message: "Architecture description is empty.".into(),
            suggestion: Some("Describe the system components and their interactions.".into()),
        });
        return MomusReview {
            target: "architecture".into(),
            review_type: ReviewType::ArchitectureReview,
            passed: false,
            comments,
            summary: "Architecture is empty.".into(),
        };
    }

    // Check for component descriptions
    let has_components =
        arch_text.to_lowercase().contains("component") || arch_text.contains("## ");
    if !has_components {
        comments.push(ReviewComment {
            severity: ReviewSeverity::Major,
            category: "completeness".into(),
            file: None,
            line: None,
            message: "No clear component/module breakdown found.".into(),
            suggestion: Some(
                "Use sections (## ComponentName) to document each major component.".into(),
            ),
        });
    }

    // Check for dependency descriptions
    let has_deps = arch_text.contains("-->")
        || arch_text.contains("->")
        || arch_text.contains("depends")
        || arch_text.contains("uses");
    if !has_deps {
        comments.push(ReviewComment {
            severity: ReviewSeverity::Major,
            category: "completeness".into(),
            file: None,
            line: None,
            message: "No dependency or data-flow information found.".into(),
            suggestion: Some(
                "Document how components interact (-> for dependency direction).".into(),
            ),
        });
    }

    // 2. Semantic review (LLM)
    let llm_result = client
        .chat(ARCH_REVIEW_SYSTEM_PROMPT, arch_text, true)
        .await;

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
                    file: None,
                    line: None,
                    message: format!("Semantic review failed to parse: {e}"),
                    suggestion: None,
                });
                (
                    true,
                    "Semantic review skipped due to parsing error.".to_string(),
                    vec![],
                )
            }
        },
        Err(e) => {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Minor,
                category: "system".into(),
                file: None,
                line: None,
                message: format!("Semantic review failed: {e}"),
                suggestion: None,
            });
            (
                true,
                "Semantic review skipped due to LLM error.".to_string(),
                vec![],
            )
        }
    };

    comments.extend(llm_comments);

    let passed = comments
        .iter()
        .all(|c| c.severity != ReviewSeverity::Blocker && c.severity != ReviewSeverity::Critical)
        && llm_passed;

    let summary = if passed {
        format!(
            "✅ Architecture review PASSED ({} suggestions). Summary: {}",
            comments.len(),
            llm_summary
        )
    } else {
        format!(
            "❌ Architecture review FAILED ({} issues). Summary: {}",
            comments.len(),
            llm_summary
        )
    };

    MomusReview {
        target: "architecture".into(),
        review_type: ReviewType::ArchitectureReview,
        passed,
        comments,
        summary,
    }
}
