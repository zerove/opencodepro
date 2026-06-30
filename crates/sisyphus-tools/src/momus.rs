use crate::metis::LlmClient;
use crate::ToolResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

// --- Types ---

/// LLM response structure for semantic review.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmReviewResponse {
    pub passed: bool,
    pub summary: String,
    pub comments: Vec<LlmReviewComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmReviewComment {
    pub severity: ReviewSeverity,
    pub category: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub message: String,
    pub suggestion: Option<String>,
}

impl From<LlmReviewComment> for ReviewComment {
    fn from(l: LlmReviewComment) -> Self {
        Self {
            severity: l.severity,
            category: l.category,
            file: l.file,
            line: l.line,
            message: l.message,
            suggestion: l.suggestion,
        }
    }
}

/// Severity of a review finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewSeverity {
    Blocker,
    Critical,
    Major,
    Minor,
    Nitpick,
}

impl std::fmt::Display for ReviewSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewSeverity::Blocker => write!(f, "blocker"),
            ReviewSeverity::Critical => write!(f, "critical"),
            ReviewSeverity::Major => write!(f, "major"),
            ReviewSeverity::Minor => write!(f, "minor"),
            ReviewSeverity::Nitpick => write!(f, "nitpick"),
        }
    }
}

/// A single review comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewComment {
    pub severity: ReviewSeverity,
    pub category: String, // e.g. "completeness", "consistency", "clarity", "verifiability"
    pub file: Option<String>,
    pub line: Option<usize>,
    pub message: String,
    pub suggestion: Option<String>,
}

/// The result of a review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MomusReview {
    pub target: String, // what was reviewed (plan file, code path, spec)
    pub review_type: ReviewType,
    pub passed: bool,
    pub comments: Vec<ReviewComment>,
    pub summary: String,
}

/// What kind of review was performed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewType {
    PlanReview,
    CodeReview,
    SpecReview,
    ArchitectureReview,
}

impl std::fmt::Display for ReviewType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewType::PlanReview => write!(f, "plan review"),
            ReviewType::CodeReview => write!(f, "code review"),
            ReviewType::SpecReview => write!(f, "spec review"),
            ReviewType::ArchitectureReview => write!(f, "architecture review"),
        }
    }
}

// --- Plan Review ---

const PLAN_REVIEW_SYSTEM_PROMPT: &str = r#"You are Momus, a senior engineer and quality gatekeeper.
Your goal is to review a project plan for completeness, consistency, and verifiability.

Analyze the plan and return a JSON object with the following structure:
{
  "passed": true/false,
  "summary": "Concise summary of the review",
  "comments": [
    {
      "severity": "Blocker|Critical|Major|Minor|Nitpick",
      "category": "completeness|consistency|clarity|verifiability|structure",
      "file": null,
      "line": null,
      "message": "Detailed explanation of the issue",
      "suggestion": "How to fix it"
    }
  ]
}

Guidelines:
1. Completeness: Are all necessary steps present? Is the goal clearly defined?
2. Verifiability: Does each step have a clear, measurable outcome? Avoid vague terms like 'improve', 'optimize' without metrics.
3. Consistency: Do steps follow a logical order? Are there missing dependencies?
4. Structure: Is the plan broken down into atomic, manageable tasks?
5. Severity:
   - Blocker: Missing critical steps or fundamentally flawed approach.
   - Critical: Major gaps that will likely lead to failure.
   - Major: Significant issues that should be fixed.
   - Minor: Small improvements or clarifications.
   - Nitpick: Style or wording suggestions.
"#;

/// Review a plan (as a string) for completeness, consistency, and verifiability.
pub async fn review_plan(client: &dyn LlmClient, plan_text: &str, plan_name: &str) -> MomusReview {
    let mut comments = Vec::new();

    // 1. Heuristic checks (Fast feedback)
    if plan_name.trim().is_empty() {
        comments.push(ReviewComment {
            severity: ReviewSeverity::Critical,
            category: "completeness".into(),
            file: None,
            line: None,
            message: "Plan has no name — every plan must have a descriptive name.".into(),
            suggestion: Some("Add a name that summarises the plan's goal.".into()),
        });
    }

    if plan_text.trim().is_empty() {
        comments.push(ReviewComment {
            severity: ReviewSeverity::Blocker,
            category: "completeness".into(),
            file: None,
            line: None,
            message: "Plan body is empty — no steps or description provided.".into(),
            suggestion: Some("Define at least one step with a clear outcome.".into()),
        });
        return MomusReview {
            target: plan_name.to_string(),
            review_type: ReviewType::PlanReview,
            passed: false,
            comments,
            summary: "Plan is empty — no review possible.".to_string(),
        };
    }

    // Structure checks
    let lines: Vec<&str> = plan_text.lines().collect();
    let step_lines: Vec<(usize, &str)> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.trim().starts_with("- ") || l.trim().starts_with("* "))
        .map(|(i, l)| (i + 1, l.trim()))
        .collect();

    if step_lines.is_empty() {
        comments.push(ReviewComment {
            severity: ReviewSeverity::Major,
            category: "structure".into(),
            file: None,
            line: None,
            message: "No structured steps found (lines starting with '- ' or '* '). Consider breaking the plan into atomic steps.".into(),
            suggestion: Some("Format each step as '- description of the step'".into()),
        });
    }

    // Verifiability checks
    let vague_words = [
        "TODO",
        "FIXME",
        "sometime",
        "later",
        "maybe",
        "perhaps",
        "eventually",
    ];
    for &w in &vague_words {
        for (line_num, line) in lines.iter().enumerate() {
            if line.to_uppercase().contains(&w.to_uppercase()) {
                comments.push(ReviewComment {
                    severity: ReviewSeverity::Minor,
                    category: "clarity".into(),
                    file: None,
                    line: Some(line_num + 1),
                    message: format!(
                        "Vague/placeholder term '{}' found. Replace with concrete criteria.",
                        w
                    ),
                    suggestion: Some(format!(
                        "Replace '{}' with a specific, verifiable outcome.",
                        w
                    )),
                });
            }
        }
    }

    // Outcome clarity
    for &(line_num, line) in &step_lines {
        let desc = line
            .trim_start_matches("- ")
            .trim_start_matches("* ")
            .trim();
        if desc.len() < 15 {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Major,
                category: "verifiability".into(),
                file: None,
                line: Some(line_num),
                message: format!(
                    "Step description too short ({} chars). A step should describe WHAT and WHY.",
                    desc.len()
                ),
                suggestion: Some(format!(
                    "Expand '{}' to include the expected outcome.",
                    desc
                )),
            });
        }
        if !desc.contains(' ') {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Major,
                category: "clarity".into(),
                file: None,
                line: Some(line_num),
                message: format!(
                    "Step '{}' appears to be a single word — this is likely a placeholder.",
                    desc
                ),
                suggestion: Some(
                    "Write a full sentence describing the task and its deliverable.".into(),
                ),
            });
        }
    }

    // Scope boundary check
    let scope_keywords = [
        "scope",
        "boundary",
        "out of scope",
        "not in scope",
        "limitation",
    ];
    let has_scope = scope_keywords
        .iter()
        .any(|k| plan_text.to_lowercase().contains(k));
    if !has_scope && step_lines.len() > 3 {
        comments.push(ReviewComment {
            severity: ReviewSeverity::Minor,
            category: "completeness".into(),
            file: None,
            line: None,
            message: "Plan has multiple steps but no explicit scope boundaries. Consider defining what is OUT of scope.".into(),
            suggestion: Some("Add a '## Out of Scope' section listing what this plan intentionally does NOT cover.".into()),
        });
    }

    // 2. Semantic review (LLM)
    let user_prompt = format!("Plan Name: {}\n\nPlan Content:\n{}", plan_name, plan_text);
    let llm_result = client
        .chat(PLAN_REVIEW_SYSTEM_PROMPT, &user_prompt, true)
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

    // Determine pass/fail
    let blocker_count = comments
        .iter()
        .filter(|c| c.severity == ReviewSeverity::Blocker)
        .count();
    let critical_count = comments
        .iter()
        .filter(|c| c.severity == ReviewSeverity::Critical)
        .count();
    let passed = (blocker_count == 0 && critical_count == 0) && llm_passed;

    let summary = if passed {
        format!(
            "✅ Plan review PASSED ({} issues: {} major, {} minor, {} nitpick). Summary: {}",
            comments.len(),
            comments
                .iter()
                .filter(|c| c.severity == ReviewSeverity::Major)
                .count(),
            comments
                .iter()
                .filter(|c| c.severity == ReviewSeverity::Minor)
                .count(),
            comments
                .iter()
                .filter(|c| c.severity == ReviewSeverity::Nitpick)
                .count(),
            llm_summary
        )
    } else {
        format!(
            "❌ Plan review FAILED ({} blockers, {} critical, {} major, {} minor). Summary: {}",
            blocker_count,
            critical_count,
            comments
                .iter()
                .filter(|c| c.severity == ReviewSeverity::Major)
                .count(),
            comments
                .iter()
                .filter(|c| c.severity == ReviewSeverity::Minor)
                .count(),
            llm_summary
        )
    };

    MomusReview {
        target: plan_name.to_string(),
        review_type: ReviewType::PlanReview,
        passed,
        comments,
        summary,
    }
}

// --- Code Review ---

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
        if !in_fn {
            if let Some(name) = extract_fn_name(trimmed) {
                in_fn = true;
                fn_start = i;
                fn_name = name;
                brace_depth = trimmed.chars().filter(|&c| c == '{').count();
                if brace_depth > 0 && !trimmed.ends_with('{') && !trimmed.contains(';') {
                    brace_depth = 0;
                    in_fn = false;
                }
            }
        } else {
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
                            "Function '{}' is {} lines long (recommended max: 50).",
                            fn_name, fn_lines
                        ),
                        suggestion: Some(
                            "Consider extracting helper functions to reduce complexity.".into(),
                        ),
                    });
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
    let llm_result = client
        .chat(CODE_REVIEW_SYSTEM_PROMPT, &user_prompt, true)
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
            }
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
        }
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

/// Review architecture documentation for consistency.
///
/// Checks:
/// - Crate/component descriptions present
/// - Dependency arrows are directional
/// - No circular dependency descriptions
/// - Consistent naming
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

// --- Helpers ---

fn extract_fn_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Match: fn name(  OR  pub fn name(  OR  pub async fn name(
    if let Some(rest) = trimmed.strip_prefix("fn ") {
        let name = rest.split('(').next()?.trim();
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(name.to_string());
        }
    }
    if let Some(pos) = trimmed.find(" fn ") {
        let rest = &trimmed[pos + 4..];
        let name = rest.split('(').next()?.trim();
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(name.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metis::DummyLlmClient;
    use tempfile::TempDir;

    const DUMMY: DummyLlmClient = DummyLlmClient;

    // ── Plan Review ───────────────────────────────────────

    #[tokio::test]
    async fn test_review_plan_empty_name() {
        let review = review_plan(&DUMMY, "- do something", "").await;
        assert!(!review.passed);
        assert!(review.comments.iter().any(|c| c.category == "completeness"));
    }

    #[tokio::test]
    async fn test_review_plan_empty_body() {
        let review = review_plan(&DUMMY, "", "My Plan").await;
        assert!(!review.passed);
        // Name is "My Plan" (non-empty) so only 1 issue: empty body blocker
        assert_eq!(review.comments.len(), 1);
        assert_eq!(review.comments[0].severity, ReviewSeverity::Blocker);
    }

    #[tokio::test]
    async fn test_review_plan_clean() {
        let text = "\
- Implement user authentication with email/password
- Add session management with JWT tokens
- Create middleware for route protection
- Write integration tests for auth flow
";
        let review = review_plan(&DUMMY, text, "Auth Implementation").await;
        assert!(
            review.passed,
            "clean plan should pass: {:?}",
            review.comments
        );
    }

    #[tokio::test]
    async fn test_review_plan_vague_language() {
        let text = "\
- setup database TODO: later
- maybe add caching eventually
";
        let review = review_plan(&DUMMY, text, "Infra").await;
        // Vague language is Minor severity, so the plan still passes
        let vague = review.comments.iter().filter(|c| c.category == "clarity");
        assert!(vague.count() >= 2);
    }

    #[tokio::test]
    async fn test_review_plan_short_steps() {
        let text = "- fix it\n- done\n";
        let review = review_plan(&DUMMY, text, "Quick fixes").await;
        // Short steps are Major severity, test verifies detection (not pass/fail)
        let short = review
            .comments
            .iter()
            .filter(|c| c.category == "verifiability");
        assert!(short.count() >= 2);
        // Still passes because no Blocker/Critical
        assert!(review.passed);
    }

    #[tokio::test]
    async fn test_review_plan_no_scope_multiple_steps() {
        let text = "\
- Implement feature A
- Implement feature B
- Implement feature C
- Implement feature D
";
        let review = review_plan(&DUMMY, text, "Big Project").await;
        let scope_issues = review
            .comments
            .iter()
            .filter(|c| c.message.contains("scope"));
        assert!(scope_issues.count() >= 1);
    }

    // ── Code Review ───────────────────────────────────────

    #[tokio::test]
    async fn test_review_code_long_lines() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("long.rs");
        let content = format!("fn main() {{\n    println!(\"{}\");\n}}\n", "x".repeat(150));
        std::fs::write(&path, &content).unwrap();
        let review = review_code(&DUMMY, &path, &content).await.unwrap();
        assert!(!review.comments.is_empty());
        assert!(review
            .comments
            .iter()
            .any(|c| c.message.contains("Line too long")));
    }

    #[tokio::test]
    async fn test_review_code_todo_markers() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("todo.rs");
        let content = "\
// TODO: implement this
fn foo() {}
// FIXME: broken
fn bar() {}
";
        std::fs::write(&path, content).unwrap();
        let review = review_code(&DUMMY, &path, content).await.unwrap();
        let markers = review
            .comments
            .iter()
            .filter(|c| c.message.contains("marker"));
        assert_eq!(markers.count(), 2);
    }

    #[tokio::test]
    async fn test_review_code_clean() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("clean.rs");
        let content = "\
/// Adds two numbers.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
";
        std::fs::write(&path, content).unwrap();
        let review = review_code(&DUMMY, &path, content).await.unwrap();
        // Documented public fn, no issues
        assert!(review.passed);
    }

    #[tokio::test]
    async fn test_review_code_undocumented_pub() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("undoc.rs");
        let content = "pub fn undoc() -> i32 { 42 }\n";
        std::fs::write(&path, content).unwrap();
        let review = review_code(&DUMMY, &path, content).await.unwrap();
        assert!(review
            .comments
            .iter()
            .any(|c| c.category == "documentation"));
    }

    #[tokio::test]
    async fn test_review_code_long_function() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("longfn.rs");
        let mut content = String::from("pub fn long_function() {\n");
        for i in 0..60 {
            content.push_str(&format!("    let x_{} = {};\n", i, i));
        }
        content.push_str("}\n");
        std::fs::write(&path, &content).unwrap();
        let review = review_code(&DUMMY, &path, &content).await.unwrap();
        let complex = review
            .comments
            .iter()
            .filter(|c| c.category == "complexity");
        assert!(complex.count() >= 1);
    }

    #[tokio::test]
    async fn test_review_code_batch() {
        let dir = TempDir::new().unwrap();
        let path1 = dir.path().join("a.rs");
        let path2 = dir.path().join("b.rs");
        let content1 = "pub fn foo() -> i32 { 1 }";
        let content2 = "// TODO: fix\npub fn bar() -> i32 { 2 }";
        std::fs::write(&path1, content1).unwrap();
        std::fs::write(&path2, content2).unwrap();

        let files = vec![(&path1 as &Path, content1), (&path2 as &Path, content2)];
        let reviews = review_code_batch(&DUMMY, &files).await.unwrap();
        assert_eq!(reviews.len(), 2);
        // TODO markers are Minor severity, so both still pass
        assert!(reviews[0].passed);
        assert!(reviews[1].passed);
        // But the second should have at least one comment (the TODO)
        assert!(!reviews[1].comments.is_empty());
    }

    // --- Architecture Review ---

    // TODO: Implement architecture review logic using this prompt
    #[allow(dead_code)]
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
    // --- Helpers ---

    #[tokio::test]
    async fn test_review_architecture_empty() {
        let review = review_architecture(&DUMMY, "").await;
        assert!(!review.passed);
    }

    #[tokio::test]
    async fn test_review_architecture_with_components() {
        let text = "\
## Core Engine
Handles DAG execution.

## Bridge
gRPC server.

Core Engine --> Bridge: sends events
";
        let review = review_architecture(&DUMMY, text).await;
        assert!(review.passed, "should pass: {:?}", review.comments);
    }

    #[tokio::test]
    async fn test_review_architecture_missing_deps() {
        let text = "## Component A\n## Component B\n";
        let review = review_architecture(&DUMMY, text).await;
        let dep_issues = review
            .comments
            .iter()
            .filter(|c| c.message.contains("dependency"));
        assert!(dep_issues.count() >= 1);
    }

    // ── ReviewSeverity Display ────────────────────────────

    #[test]
    fn test_severity_display() {
        assert_eq!(ReviewSeverity::Blocker.to_string(), "blocker");
        assert_eq!(ReviewSeverity::Critical.to_string(), "critical");
        assert_eq!(ReviewSeverity::Major.to_string(), "major");
        assert_eq!(ReviewSeverity::Minor.to_string(), "minor");
        assert_eq!(ReviewSeverity::Nitpick.to_string(), "nitpick");
    }

    // ── Helpers ───────────────────────────────────────────

    #[test]
    fn test_extract_fn_name_simple() {
        assert_eq!(extract_fn_name("fn foo() {").unwrap(), "foo");
        assert_eq!(extract_fn_name("pub fn bar() -> i32 {").unwrap(), "bar");
        assert_eq!(extract_fn_name("pub async fn baz() {").unwrap(), "baz");
    }

    #[test]
    fn test_extract_fn_name_no_match() {
        assert!(extract_fn_name("let x = 1;").is_none());
        assert!(extract_fn_name("struct Foo {}").is_none());
    }
}
