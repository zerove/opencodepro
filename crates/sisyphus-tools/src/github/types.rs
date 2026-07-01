//! Shared types for the GitHub module.

use serde::{Deserialize, Serialize};

/// Workflow kind for template generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowKind {
    /// CI workflow: build, lint, test
    Ci,
    /// PR review workflow: runs Momus + Oracle review pipeline
    PrReview,
    /// Release workflow: build release artifacts and publish
    Release,
}

impl WorkflowKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowKind::Ci => "ci",
            WorkflowKind::PrReview => "pr-review",
            WorkflowKind::Release => "release",
        }
    }
}

/// Options for workflow generation.
#[derive(Debug, Clone, Default)]
pub struct GenerateOptions {
    /// Rust toolchain (default: stable)
    pub rust_toolchain: String,
    /// Extra flags passed to cargo commands
    pub cargo_extra_flags: String,
    /// Exclude packages from CI (comma-separated)
    pub exclude_packages: String,
    /// Whether to include clippy in CI
    pub include_clippy: bool,
    /// Whether to include cargo-deny audit
    pub include_deny: bool,
}

/// CI-mode configuration read from environment variables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CiConfig {
    /// Set to "1" or "true" to enable non-interactive CI mode.
    pub ci_mode: bool,
    /// GitHub token for API calls
    pub github_token: Option<String>,
    /// PR number being reviewed (if applicable)
    pub pr_number: Option<u64>,
    /// Output format: "plain" (default) or "json"
    pub output_format: String,
}

impl CiConfig {
    /// Load CI config from the environment.
    pub fn from_env() -> Self {
        Self {
            ci_mode: std::env::var("SISYPHUS_CI")
                .is_ok_and(|v| v == "1" || v.to_lowercase() == "true"),
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            pr_number: std::env::var("PR_NUMBER")
                .ok()
                .and_then(|v| v.parse().ok()),
            output_format: std::env::var("SISYPHUS_OUTPUT_FORMAT")
                .unwrap_or_else(|_| "plain".into()),
        }
    }
}
