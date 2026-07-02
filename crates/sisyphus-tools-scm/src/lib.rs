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
pub use patch::apply_patch;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_content_deterministic() {
        let content = "hello world";
        let h1 = hashline::hash_content(content);
        let h2 = hashline::hash_content(content);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_parse_hashline_basic() {
        let line = "some code // LINE#42:a1b2c3d4";
        let data = hashline::parse_hashline(line).expect("Should parse");
        assert_eq!(data.line_number, 42);
        assert_eq!(data.content_hash, "a1b2c3d4");
    }
}

