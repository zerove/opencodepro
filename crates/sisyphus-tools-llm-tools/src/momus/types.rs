//! Shared types for Momus review system.

use serde::{Deserialize, Serialize};

// --- Types ---

/// LLM response structure for semantic review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LlmReviewResponse {
    pub passed: bool,
    pub summary: String,
    pub comments: Vec<LlmReviewComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LlmReviewComment {
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
