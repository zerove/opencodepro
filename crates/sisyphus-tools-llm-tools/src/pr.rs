use serde::{Deserialize, Serialize};
use sisyphus_tools_core::ToolResult;
use sisyphus_tools_llm::metis::LlmClient;
use std::sync::Arc;

/// Representation of a GitHub Pull Request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub url: String,
    pub state: String,
    #[serde(rename = "headRefName")]
    pub head_branch: String,
    #[serde(rename = "baseRefName")]
    pub base_branch: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// Manager for GitHub PR workflows using the `gh` CLI.
pub struct PrManager;

impl PrManager {
    /// Lists open pull requests in the current repository.
    pub fn list_prs() -> ToolResult<Vec<PullRequest>> {
        let output = Self::run_gh(&[
            "pr",
            "list",
            "--json",
            "number,title,author,createdAt,headRefName,baseRefName,state,url",
        ])?;
        Self::parse_pr_list(&output)
    }

    /// Parses the JSON output from `gh pr list`.
    pub fn parse_pr_list(json: &str) -> ToolResult<Vec<PullRequest>> {
        let raw_prs: Vec<serde_json::Value> = serde_json::from_str(json).map_err(|e| {
            sisyphus_tools_core::ToolError::Generic(format!("Failed to parse PR list JSON: {e}"))
        })?;

        let mut prs = Vec::new();
        for val in raw_prs {
            let author = val["author"]["login"].as_str().unwrap_or("unknown").to_string();

            prs.push(PullRequest {
                number: val["number"].as_u64().unwrap_or(0),
                title: val["title"].as_str().unwrap_or("").to_string(),
                author,
                url: val["url"].as_str().unwrap_or("").to_string(),
                state: val["state"].as_str().unwrap_or("unknown").to_string(),
                head_branch: val["headRefName"].as_str().unwrap_or("").to_string(),
                base_branch: val["baseRefName"].as_str().unwrap_or("").to_string(),
                created_at: val["createdAt"].as_str().unwrap_or("").to_string(),
            });
        }

        Ok(prs)
    }

    /// Checks out a specific pull request branch.
    pub fn checkout_pr(number: u64) -> ToolResult<()> {
        let num_str = number.to_string();
        Self::run_gh(&["pr", "checkout", &num_str])?;

        // Show recent log to confirm what was pulled
        let log = sisyphus_tools_core::process::run_command_blocking(
            "git",
            ["log", "--oneline", "-5"],
            sisyphus_tools_core::process::CommandOpts::default(),
        )?;

        if !log.stdout.is_empty() {
            tracing::info!(recent_commits = %log.stdout, "checked out PR");
        }

        Ok(())
    }

    /// Reviews a pull request using an LLM.
    pub async fn review_pr(number: u64, llm_client: Arc<dyn LlmClient>) -> ToolResult<String> {
        let num_str = number.to_string();

        // 1. Checkout the PR first to ensure we are on the right context
        Self::checkout_pr(number)?;

        // 2. Get the diff
        let diff = Self::run_gh(&["pr", "diff", &num_str])?;

        if diff.trim().is_empty() {
            return Err(sisyphus_tools_core::ToolError::Generic(
                "PR diff is empty".into(),
            ));
        }

        // 3. Run review pipeline (Momus-style)
        let system_prompt = r"You are Momus, a senior engineer and quality gatekeeper.
Your goal is to perform a deep semantic review of the provided GitHub PR diff.

Analyze the diff and return a structured review.
Focus on:
1. Logic: Edge cases, potential bugs, and correctness.
2. Performance: Inefficient algorithms or unnecessary allocations.
3. Maintainability: Readability and adherence to SRP.
4. Security: Common vulnerabilities.

Return your response as a clear, professional review with:
- A summary of the changes.
- A verdict (PASSED/FAILED).
- A list of specific issues with severity (Blocker, Critical, Major, Minor, Nitpick) and suggestions for improvement.
";

        let user_prompt = format!("PR #{number} Diff:\n\n{diff}");

        let review = llm_client.chat(system_prompt, &user_prompt, false).await.map_err(|e| {
            sisyphus_tools_core::ToolError::Generic(format!("LLM review failed: {e}"))
        })?;

        Ok(review)
    }

    /// Helper to run `gh` commands with error handling for missing binary.
    fn run_gh(args: &[&str]) -> ToolResult<String> {
        let output = sisyphus_tools_core::process::run_command_blocking(
            "gh",
            args,
            sisyphus_tools_core::process::CommandOpts::default(),
        )
        .map_err(|e| match e {
            sisyphus_tools_core::ToolError::Generic(m) => {
                sisyphus_tools_core::ToolError::Generic(format!(
                    "{m}\nPR workflow requires GitHub CLI (gh). Install: https://cli.github.com/"
                ))
            },
            other => other,
        })?;

        if output.exit_code == 0 {
            Ok(output.stdout.clone())
        } else {
            let stderr = output.stderr.clone();
            Err(sisyphus_tools_core::ToolError::CommandFailed {
                code: output.exit_code,
                stdout: output.stdout.clone(),
                stderr,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pr_list_valid_json() {
        let json = r#"[
            {
                "number": 123,
                "title": "Fix critical bug in core",
                "author": { "login": "octocat" },
                "url": "https://github.com/owner/repo/pull/123",
                "state": "OPEN",
                "headRefName": "fix/bug-123",
                "baseRefName": "main",
                "createdAt": "2023-10-01T10:00:00Z"
            },
            {
                "number": 456,
                "title": "Add new feature X",
                "author": { "login": "dev-user" },
                "url": "https://github.com/owner/repo/pull/456",
                "state": "OPEN",
                "headRefName": "feat/feature-x",
                "baseRefName": "develop",
                "createdAt": "2023-10-02T12:00:00Z"
            }
        ]"#;

        let prs = PrManager::parse_pr_list(json).unwrap();
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].number, 123);
        assert_eq!(prs[0].title, "Fix critical bug in core");
        assert_eq!(prs[0].author, "octocat");
        assert_eq!(prs[1].number, 456);
        assert_eq!(prs[1].author, "dev-user");
    }

    #[test]
    fn test_parse_pr_list_invalid_json() {
        let json = r#"[ { "number": "not-a-number" } ]"#;
        let result = PrManager::parse_pr_list(json);
        // Should not panic, but might have default values or error
        // In our implementation, we use unwrap_or(0), so it should succeed with 0
        let prs = result.unwrap();
        assert_eq!(prs[0].number, 0);
    }

    #[test]
    fn test_parse_pr_list_empty() {
        let json = "[]";
        let prs = PrManager::parse_pr_list(json).unwrap();
        assert!(prs.is_empty());
    }
}
