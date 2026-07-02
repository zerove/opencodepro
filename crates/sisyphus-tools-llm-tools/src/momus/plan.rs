//! Plan review — heuristic checks + LLM semantic review for completeness,
//! consistency, and verifiability.

use super::types::{LlmReviewResponse, MomusReview, ReviewComment, ReviewSeverity, ReviewType};
use sisyphus_tools_llm::metis::LlmClient;

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
                        "Vague/placeholder term '{w}' found. Replace with concrete criteria."
                    ),
                    suggestion: Some(format!(
                        "Replace '{w}' with a specific, verifiable outcome."
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
                    "Expand '{desc}' to include the expected outcome."
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
                    "Step '{desc}' appears to be a single word — this is likely a placeholder."
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
    let user_prompt = format!("Plan Name: {plan_name}\n\nPlan Content:\n{plan_text}");
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

