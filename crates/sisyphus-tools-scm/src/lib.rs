//! Source Control Management tools for the Sisyphus tool ecosystem.
//!
//! Provides git operations, GitHub integration, hashline tracking, and patch management.

pub mod git;
pub mod github;
pub mod hashline;
pub mod patch;

pub use git::{
    BranchInfo, CommitEntry, FileStatus, GitCommitLog, GitStatus, add, branch, checkout, commit,
    current_branch, diff, init, last_commit_diff, log, pull, push, status, summary,
};
pub use github::{
    CiConfig, GenerateOptions, WorkflowKind, ci_environment, generate_workflow, is_ci,
    post_pr_comment, validate_workflow_yaml, write_workflow,
};
pub use hashline::*;
pub use patch::*;
