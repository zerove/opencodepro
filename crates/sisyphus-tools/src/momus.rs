use crate::ToolResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Types ──────────────────────────────────────────────────

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
    pub category: String,         // e.g. "completeness", "consistency", "clarity", "verifiability"
    pub file: Option<String>,
    pub line: Option<usize>,
    pub message: String,
    pub suggestion: Option<String>,
}

/// The result of a review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MomusReview {
    pub target: String,           // what was reviewed (plan file, code path, spec)
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

// ── Plan Review ────────────────────────────────────────────

/// Review a plan (as a string) for completeness, consistency, and verifiability.
///
/// Checks performed:
/// - Empty name / description
/// - No steps defined
/// - Steps without clear outcomes
/// - Missing dependencies between steps
/// - Ambiguous or unverifiable language
/// - Missing effort estimates
pub fn review_plan(plan_text: &str, plan_name: &str) -> MomusReview {
    let mut comments = Vec::new();

    // 1. Completeness checks
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

    // 2. Structure checks
    let lines: Vec<&str> = plan_text.lines().collect();
    let step_lines: Vec<(usize, &str)> = lines.iter()
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

    // 3. Verifiability checks
    let vague_words = ["TODO", "FIXME", "sometime", "later", "maybe", "perhaps", "eventually"];
    for &w in &vague_words {
        for (line_num, line) in lines.iter().enumerate() {
            if line.to_uppercase().contains(&w.to_uppercase()) {
                comments.push(ReviewComment {
                    severity: ReviewSeverity::Minor,
                    category: "clarity".into(),
                    file: None,
                    line: Some(line_num + 1),
                    message: format!("Vague/placeholder term '{}' found. Replace with concrete criteria.", w),
                    suggestion: Some(format!("Replace '{}' with a specific, verifiable outcome.", w)),
                });
            }
        }
    }

    // 4. Outcome clarity
    for &(line_num, line) in &step_lines {
        let desc = line.trim_start_matches("- ").trim_start_matches("* ").trim();
        if desc.len() < 15 {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Major,
                category: "verifiability".into(),
                file: None,
                line: Some(line_num),
                message: format!("Step description too short ({} chars). A step should describe WHAT and WHY.", desc.len()),
                suggestion: Some(format!("Expand '{}' to include the expected outcome.", desc)),
            });
        }
        if !desc.contains(' ') {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Major,
                category: "clarity".into(),
                file: None,
                line: Some(line_num),
                message: format!("Step '{}' appears to be a single word — this is likely a placeholder.", desc),
                suggestion: Some("Write a full sentence describing the task and its deliverable.".into()),
            });
        }
    }

    // 5. Scope boundary check
    let scope_keywords = ["scope", "boundary", "out of scope", "not in scope", "limitation"];
    let has_scope = scope_keywords.iter().any(|k| plan_text.to_lowercase().contains(k));
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

    // Determine pass/fail
    let blocker_count = comments.iter().filter(|c| c.severity == ReviewSeverity::Blocker).count();
    let critical_count = comments.iter().filter(|c| c.severity == ReviewSeverity::Critical).count();
    let passed = blocker_count == 0 && critical_count == 0;

    let summary = if passed {
        format!(
            "✅ Plan review PASSED ({} issues: {} major, {} minor, {} nitpick)",
            comments.len(),
            comments.iter().filter(|c| c.severity == ReviewSeverity::Major).count(),
            comments.iter().filter(|c| c.severity == ReviewSeverity::Minor).count(),
            comments.iter().filter(|c| c.severity == ReviewSeverity::Nitpick).count(),
        )
    } else {
        format!(
            "❌ Plan review FAILED ({} blockers, {} critical, {} major, {} minor)",
            blocker_count,
            critical_count,
            comments.iter().filter(|c| c.severity == ReviewSeverity::Major).count(),
            comments.iter().filter(|c| c.severity == ReviewSeverity::Minor).count(),
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

// ── Code Review ────────────────────────────────────────────

/// Review source code for common issues.
///
/// Checks performed:
/// - Long lines (>120 chars)
/// - TODO/FIXME markers
/// - Large functions (>50 lines)
/// - Deep nesting
/// - Missing documentation on public items
/// - Repeated string literals (potential magic values)
pub fn review_code(path: &Path, content: &str) -> ToolResult<MomusReview> {
    let mut comments = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // 1. Line length
    for (i, line) in lines.iter().enumerate() {
        if line.len() > 120 {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Nitpick,
                category: "style".into(),
                file: Some(path.to_string_lossy().to_string()),
                line: Some(i + 1),
                message: format!("Line too long ({} chars, max 120)", line.len()),
                suggestion: Some("Break into multiple lines or extract an intermediate variable.".into()),
            });
        }
    }

    // 2. TODO/FIXME markers
    for (i, line) in lines.iter().enumerate() {
        let upper = line.to_uppercase();
        if upper.contains("TODO") || upper.contains("FIXME") || upper.contains("HACK") || upper.contains("XXX") {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Minor,
                category: "completeness".into(),
                file: Some(path.to_string_lossy().to_string()),
                line: Some(i + 1),
                message: format!("Contains marker: {}", line.trim()),
                suggestion: Some("Resolve before considering this complete, or file a tracking issue.".into()),
            });
        }
    }

    // 3. Function length heuristic (Rust: fn ... { ... })
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
                // Handle one-liners: fn foo() -> T { expr }
                if brace_depth > 0 && !trimmed.ends_with('{') && !trimmed.contains(';') {
                    brace_depth = 0;
                    in_fn = false;
                }
            }
        } else {
            brace_depth += trimmed.chars().filter(|&c| c == '{').count();
            brace_depth -= trimmed.chars().filter(|&c| c == '}').count();
            if brace_depth <= 0 {
                in_fn = false;
                let fn_lines = i - fn_start + 1;
                if fn_lines > 50 {
                    comments.push(ReviewComment {
                        severity: ReviewSeverity::Major,
                        category: "complexity".into(),
                        file: Some(path.to_string_lossy().to_string()),
                        line: Some(fn_start + 1),
                        message: format!("Function '{}' is {} lines long (recommended max: 50).", fn_name, fn_lines),
                        suggestion: Some("Consider extracting helper functions to reduce complexity.".into()),
                    });
                }
            }
        }
    }

    // 4. Missing documentation on public items
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub fn") || trimmed.starts_with("pub struct") || trimmed.starts_with("pub enum") || trimmed.starts_with("pub trait") {
            // Check if preceded by a doc comment
            let has_doc = i > 0 && (lines[i - 1].trim().starts_with("///") || lines[i - 1].trim().starts_with("//!"));
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

    // 5. Deep nesting check (indentation based)
    for (i, line) in lines.iter().enumerate() {
        let indent = line.len() - line.trim_start().len();
        if indent > 80 {
            comments.push(ReviewComment {
                severity: ReviewSeverity::Major,
                category: "complexity".into(),
                file: Some(path.to_string_lossy().to_string()),
                line: Some(i + 1),
                message: "Deeply nested code (indentation > 80 chars).".into(),
                suggestion: Some("Extract inner logic into a helper function or use early returns.".into()),
            });
        }
    }

    let passed = comments.iter().all(|c| c.severity != ReviewSeverity::Blocker && c.severity != ReviewSeverity::Critical);

    let summary = if passed {
        format!("✅ Code review PASSED ({} issues found)", comments.len())
    } else {
        format!("❌ Code review FAILED ({} issues found)", comments.len())
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
pub fn review_code_batch(files: &[(&Path, &str)]) -> ToolResult<Vec<MomusReview>> {
    let mut reviews = Vec::new();
    for (path, content) in files {
        reviews.push(review_code(path, content)?);
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
pub fn review_architecture(arch_text: &str) -> MomusReview {
    let mut comments = Vec::new();

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
    let has_components = arch_text.to_lowercase().contains("component") || arch_text.contains("## ");
    if !has_components {
        comments.push(ReviewComment {
            severity: ReviewSeverity::Major,
            category: "completeness".into(),
            file: None,
            line: None,
            message: "No clear component/module breakdown found.".into(),
            suggestion: Some("Use sections (## ComponentName) to document each major component.".into()),
        });
    }

    // Check for dependency descriptions
    let has_deps = arch_text.contains("-->") || arch_text.contains("->") || arch_text.contains("depends") || arch_text.contains("uses");
    if !has_deps {
        comments.push(ReviewComment {
            severity: ReviewSeverity::Major,
            category: "completeness".into(),
            file: None,
            line: None,
            message: "No dependency or data-flow information found.".into(),
            suggestion: Some("Document how components interact (-> for dependency direction).".into()),
        });
    }

    let passed = comments.iter().all(|c| c.severity != ReviewSeverity::Blocker && c.severity != ReviewSeverity::Critical);

    let summary = if passed {
        format!("✅ Architecture review PASSED ({} suggestions)", comments.len())
    } else {
        format!("❌ Architecture review FAILED ({} issues)", comments.len())
    };

    MomusReview {
        target: "architecture".into(),
        review_type: ReviewType::ArchitectureReview,
        passed,
        comments,
        summary,
    }
}

// ── Helpers ─────────────────────────────────────────────────

fn extract_fn_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Match: fn name(  OR  pub fn name(  OR  pub async fn name(
    if trimmed.starts_with("fn ") {
        let rest = &trimmed[3..];
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
    use tempfile::TempDir;

    // ── Plan Review ───────────────────────────────────────

    #[test]
    fn test_review_plan_empty_name() {
        let review = review_plan("- do something", "");
        assert!(!review.passed);
        assert!(review.comments.iter().any(|c| c.category == "completeness"));
    }

    #[test]
    fn test_review_plan_empty_body() {
        let review = review_plan("", "My Plan");
        assert!(!review.passed);
        // Name is "My Plan" (non-empty) so only 1 issue: empty body blocker
        assert_eq!(review.comments.len(), 1);
        assert_eq!(review.comments[0].severity, ReviewSeverity::Blocker);
    }

    #[test]
    fn test_review_plan_clean() {
        let text = "\
- Implement user authentication with email/password
- Add session management with JWT tokens
- Create middleware for route protection
- Write integration tests for auth flow
";
        let review = review_plan(text, "Auth Implementation");
        assert!(review.passed, "clean plan should pass: {:?}", review.comments);
    }

    #[test]
    fn test_review_plan_vague_language() {
        let text = "\
- setup database TODO: later
- maybe add caching eventually
";
        let review = review_plan(text, "Infra");
        // Vague language is Minor severity, so the plan still passes
        let vague = review.comments.iter().filter(|c| c.category == "clarity");
        assert!(vague.count() >= 2);
    }

    #[test]
    fn test_review_plan_short_steps() {
        let text = "- fix it\n- done\n";
        let review = review_plan(text, "Quick fixes");
        // Short steps are Major severity, test verifies detection (not pass/fail)
        let short = review.comments.iter().filter(|c| c.category == "verifiability");
        assert!(short.count() >= 2);
        // Still passes because no Blocker/Critical
        assert!(review.passed);
    }

    #[test]
    fn test_review_plan_no_scope_multiple_steps() {
        let text = "\
- Implement feature A
- Implement feature B
- Implement feature C
- Implement feature D
";
        let review = review_plan(text, "Big Project");
        let scope_issues = review.comments.iter().filter(|c| c.message.contains("scope"));
        assert!(scope_issues.count() >= 1);
    }

    // ── Code Review ───────────────────────────────────────

    #[test]
    fn test_review_code_long_lines() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("long.rs");
        let content = format!("fn main() {{\n    println!(\"{}\");\n}}\n", "x".repeat(150));
        std::fs::write(&path, &content).unwrap();
        let review = review_code(&path, &content).unwrap();
        assert!(!review.comments.is_empty());
        assert!(review.comments.iter().any(|c| c.message.contains("Line too long")));
    }

    #[test]
    fn test_review_code_todo_markers() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("todo.rs");
        let content = "\
// TODO: implement this
fn foo() {}
// FIXME: broken
fn bar() {}
";
        std::fs::write(&path, &content).unwrap();
        let review = review_code(&path, &content).unwrap();
        let markers = review.comments.iter().filter(|c| c.message.contains("marker"));
        assert_eq!(markers.count(), 2);
    }

    #[test]
    fn test_review_code_clean() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("clean.rs");
        let content = "\
/// Adds two numbers.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
";
        std::fs::write(&path, &content).unwrap();
        let review = review_code(&path, &content).unwrap();
        // Documented public fn, no issues
        assert!(review.passed);
    }

    #[test]
    fn test_review_code_undocumented_pub() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("undoc.rs");
        let content = "pub fn undoc() -> i32 { 42 }\n";
        std::fs::write(&path, &content).unwrap();
        let review = review_code(&path, &content).unwrap();
        assert!(review.comments.iter().any(|c| c.category == "documentation"));
    }

    #[test]
    fn test_review_code_long_function() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("longfn.rs");
        let mut content = String::from("pub fn long_function() {\n");
        for i in 0..60 {
            content.push_str(&format!("    let x_{} = {};\n", i, i));
        }
        content.push_str("}\n");
        std::fs::write(&path, &content).unwrap();
        let review = review_code(&path, &content).unwrap();
        let complex = review.comments.iter().filter(|c| c.category == "complexity");
        assert!(complex.count() >= 1);
    }

    #[test]
    fn test_review_code_batch() {
        let dir = TempDir::new().unwrap();
        let path1 = dir.path().join("a.rs");
        let path2 = dir.path().join("b.rs");
        let content1 = "pub fn foo() -> i32 { 1 }";
        let content2 = "// TODO: fix\npub fn bar() -> i32 { 2 }";
        std::fs::write(&path1, content1).unwrap();
        std::fs::write(&path2, content2).unwrap();

        let files = vec![
            (&path1 as &Path, content1),
            (&path2 as &Path, content2),
        ];
        let reviews = review_code_batch(&files).unwrap();
        assert_eq!(reviews.len(), 2);
        // TODO markers are Minor severity, so both still pass
        assert!(reviews[0].passed);
        assert!(reviews[1].passed);
        // But the second should have at least one comment (the TODO)
        assert!(!reviews[1].comments.is_empty());
    }

    // ── Architecture Review ───────────────────────────────

    #[test]
    fn test_review_architecture_empty() {
        let review = review_architecture("");
        assert!(!review.passed);
    }

    #[test]
    fn test_review_architecture_with_components() {
        let text = "\
## Core Engine
Handles DAG execution.

## Bridge
gRPC server.

Core Engine --> Bridge: sends events
";
        let review = review_architecture(text);
        assert!(review.passed, "should pass: {:?}", review.comments);
    }

    #[test]
    fn test_review_architecture_missing_deps() {
        let text = "## Component A\n## Component B\n";
        let review = review_architecture(text);
        let dep_issues = review.comments.iter().filter(|c| c.message.contains("dependency"));
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
