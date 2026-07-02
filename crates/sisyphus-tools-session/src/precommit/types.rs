//! Shared types for the Precommit module.

use std::path::{Path, PathBuf};

/// Configuration for the pre-commit hook checks.
#[derive(Debug, Clone)]
pub struct PrecommitConfig {
    /// Language backend: "rust", "node", "python", "go", "java", "gradle", "fe"
    pub backend: String,
    /// Run formatting check (default: true).
    pub check_fmt: bool,
    /// Run lint check (default: true).
    pub check_lint: bool,
    /// Pass deny-warnings flag to the linter (default: true, Rust-only).
    pub lint_deny_warnings: bool,
}

impl Default for PrecommitConfig {
    fn default() -> Self {
        Self {
            backend: "rust".into(),
            check_fmt: true,
            check_lint: true,
            lint_deny_warnings: true,
        }
    }
}

pub(crate) const HOOK_NAME: &str = "pre-commit";

/// Locate the `.git/hooks` directory for a repo rooted at `repo_path`.
pub(crate) fn hooks_dir(repo_path: &Path) -> PathBuf {
    repo_path.join(".git").join("hooks")
}
