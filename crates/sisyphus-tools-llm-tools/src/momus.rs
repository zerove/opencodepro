//! Momus — review system with heuristic + LLM semantic review for plans, code,
//! and architecture documentation.
//!
//! ## Sub-modules
//! - [`types`]: Shared review types (ReviewSeverity, ReviewComment, MomusReview, ReviewType).
//! - [`plan`]: Plan review — heuristic checks + LLM semantic review.
//! - [`code`]: Code review — heuristic checks + LLM semantic review.
//! - [`arch`]: Architecture review — heuristic checks + LLM semantic review.

pub mod arch;
pub mod code;
pub mod plan;
pub mod types;

pub use arch::review_architecture;
pub use code::{review_code, review_code_batch};
pub use plan::review_plan;
pub use types::{MomusReview, ReviewComment, ReviewSeverity, ReviewType};

// --- Helpers ---

pub(super) fn extract_fn_name(line: &str) -> Option<String> {
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
    use sisyphus_tools_llm::metis::DummyLlmClient;
    use std::path::Path;
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
        let vague = review.comments.iter().filter(|c| c.category == "clarity");
        assert!(vague.count() >= 2);
    }

    #[tokio::test]
    async fn test_review_plan_short_steps() {
        let text = "- fix it\n- done\n";
        let review = review_plan(&DUMMY, text, "Quick fixes").await;
        let short = review
            .comments
            .iter()
            .filter(|c| c.category == "verifiability");
        assert!(short.count() >= 2);
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
        assert!(reviews[0].passed);
        assert!(reviews[1].passed);
        assert!(!reviews[1].comments.is_empty());
    }

    // ── Architecture Review ───────────────────────────────

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
