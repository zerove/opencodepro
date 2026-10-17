//! `ExpertReviewer` implementation backed by Oracle and Momus.
//!
//! This block used to live inside `sisyphus-core`'s reflection step. Moving it
//! here means the heuristics — "does this output look like a plan?" — sit next
//! to the tools that answer the question, and core depends only on the
//! [`ExpertReviewer`](sisyphus_types::ExpertReviewer) seam (ADR-014).
//!
//! The formatting strings are reproduced exactly. They are user-visible text
//! that lands in `ReflectionReport::suggested_improvements`, so changing them
//! would silently alter every reflection report.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use sisyphus_types::{ExpertReviewer, LlmClient};

use crate::{momus, oracle};

/// Default reviewer: file analysis plus LLM plan/code review.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultExpertReviewer;

#[async_trait]
impl ExpertReviewer for DefaultExpertReviewer {
    async fn suggestions(
        &self,
        modified_files: Vec<PathBuf>,
        results: Vec<(String, String)>,
        llm: &dyn LlmClient,
    ) -> Vec<String> {
        let mut out = Vec::new();

        // Oracle: complexity and security issues in every modified file.
        for path in &modified_files {
            if let Ok(analysis) = oracle::analyze_file(path) {
                for issue in analysis.issues {
                    out.push(format!(
                        "[{}] {} at {}:{}: {}",
                        issue.severity,
                        issue.message,
                        path.display(),
                        issue.line,
                        issue.code.as_deref().unwrap_or("")
                    ));
                }
            }
        }

        // Momus: review outputs that look like plans or like code.
        for (node_name, content) in &results {
            if node_name.contains("plan") || content.contains("## ") {
                let review = momus::review_plan(llm, content, node_name.as_str()).await;
                for comment in review.comments {
                    out.push(format!(
                        "[{}] (Plan Review) {}: {} - Suggestion: {}",
                        comment.severity,
                        comment.category,
                        comment.message,
                        comment.suggestion.as_deref().unwrap_or("N/A")
                    ));
                }
            }

            if content.contains("fn ") || content.contains("pub struct") {
                // `review_code` wants a path; this output only exists in memory.
                let path_str = format!("/virtual/{}", node_name);
                if let Ok(review) = momus::review_code(llm, Path::new(&path_str), content).await {
                    for comment in review.comments {
                        out.push(format!(
                            "[{}] (Code Review) {}: {} - Suggestion: {}",
                            comment.severity,
                            comment.category,
                            comment.message,
                            comment.suggestion.as_deref().unwrap_or("N/A")
                        ));
                    }
                }
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LLM that always fails. Both review paths run their heuristic checks
    /// *before* consulting the model, so these tests stay deterministic and do
    /// not depend on a live provider.
    struct FailingLlm;

    #[async_trait]
    impl LlmClient for FailingLlm {
        async fn chat(
            &self,
            _s: &str,
            _u: &str,
            _j: bool,
        ) -> Result<String, sisyphus_types::TypesError> {
            Err(sisyphus_types::TypesError::ToolExecutionError {
                tool_id: "test".into(),
                details: "no llm in tests".into(),
            })
        }
        async fn generate(&self, _p: &str) -> Result<String, sisyphus_types::TypesError> {
            Err(sisyphus_types::TypesError::ToolExecutionError {
                tool_id: "test".into(),
                details: "no llm in tests".into(),
            })
        }
        fn get_params_for(&self, _id: &str) -> (String, String, String) {
            (String::new(), String::new(), String::new())
        }
        fn breaker(&self) -> sisyphus_types::CircuitBreaker {
            sisyphus_types::CircuitBreaker::new(sisyphus_types::BreakerConfig::default())
        }
    }

    /// 锁定「计划」启发式与**逐字格式**。
    ///
    /// 这段逻辑原本在 `sisyphus-core` 的反思步骤里，ADR-014 阶段 4 把它整体
    /// 搬到了这里。格式串是最终写进 `ReflectionReport` 的用户可见文本，搬动
    /// 时若少写一个字段，重构就会静默改变输出——所以用断言钉住。
    #[tokio::test]
    async fn plan_heuristic_fires_and_keeps_its_exact_format() {
        let out = DefaultExpertReviewer
            .suggestions(
                vec![],
                // name contains "plan"; empty body trips momus's own heuristic
                vec![("my_plan".to_string(), String::new())],
                &FailingLlm,
            )
            .await;

        assert_eq!(
            out.len(),
            1,
            "expected exactly one plan comment, got {out:?}"
        );
        assert_eq!(
            out[0],
            "[blocker] (Plan Review) completeness: Plan body is empty — no steps or \
             description provided. - Suggestion: Define at least one step with a clear outcome."
        );
    }

    /// 名字不像计划、内容也没有 `## `，就不该触发计划评审——钉住启发式不过度触发。
    #[tokio::test]
    async fn plain_output_is_not_reviewed_as_a_plan() {
        let out = DefaultExpertReviewer
            .suggestions(
                vec![],
                vec![("notes".to_string(), "just prose".to_string())],
                &FailingLlm,
            )
            .await;
        assert!(
            out.is_empty(),
            "plain prose must not be plan-reviewed, got {out:?}"
        );
    }

    /// 锁定「代码」启发式与逐字格式（超长行 → nitpick / style）。
    #[tokio::test]
    async fn code_heuristic_fires_and_keeps_its_exact_format() {
        let long_line = "fn f() {}".to_string() + &"x".repeat(130);
        let out = DefaultExpertReviewer
            .suggestions(vec![], vec![("thing".to_string(), long_line)], &FailingLlm)
            .await;

        assert!(!out.is_empty(), "code-looking output must be reviewed");
        let style =
            out.iter().find(|s| s.contains("(Code Review)")).expect("a code review comment");
        assert!(
            style.starts_with("[nitpick] (Code Review) style: "),
            "unexpected code-review format: {style}"
        );
        assert!(
            style.contains("Line too long (139 chars, max 120)"),
            "unexpected message: {style}"
        );
    }

    /// 同一个输出同时像计划和像代码时，两条评审都要触发（两次独立判断，非 else-if）。
    #[tokio::test]
    async fn both_heuristics_can_fire_for_one_output() {
        // Needs BOTH markers: "## " (or a "plan" name) for the plan heuristic,
        // and "fn " for the code heuristic -- they are independent `if`s, not
        // `else if`, so one output can trigger both.
        let content = format!("## Plan\nfn f() {{}}{}", "y".repeat(130));
        let out = DefaultExpertReviewer
            .suggestions(vec![], vec![("a_plan".to_string(), content)], &FailingLlm)
            .await;
        assert!(
            out.iter().any(|s| s.contains("(Plan Review)")),
            "plan heuristic should fire: {out:?}"
        );
        assert!(
            out.iter().any(|s| s.contains("(Code Review)")),
            "code heuristic should fire independently: {out:?}"
        );
    }
}
