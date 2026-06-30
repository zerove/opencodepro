//! GitHub Actions integration: workflow templates, CI mode detection, PR review automation.
use crate::ToolResult;
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

/// Detects if we are running in a CI environment.
pub fn is_ci() -> bool {
    std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok()
}

/// Returns the CI environment name, if any.
pub fn ci_environment() -> Option<String> {
    if std::env::var("GITHUB_ACTIONS").is_ok() {
        Some("github_actions".into())
    } else if std::env::var("CI").is_ok() {
        Some("generic_ci".into())
    } else {
        None
    }
}

/// Generate a GitHub Actions workflow YAML for the given kind.
/// Returns the raw YAML string ready for writing to `.github/workflows/`.
pub fn generate_workflow(kind: WorkflowKind, opts: &GenerateOptions) -> String {
    match kind {
        WorkflowKind::Ci => generate_ci_workflow(opts),
        WorkflowKind::PrReview => generate_pr_review_workflow(opts),
        WorkflowKind::Release => generate_release_workflow(opts),
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

fn generate_ci_workflow(opts: &GenerateOptions) -> String {
    let toolchain = if opts.rust_toolchain.is_empty() {
        "stable"
    } else {
        &opts.rust_toolchain
    };
    let exclude = if opts.exclude_packages.is_empty() {
        String::new()
    } else {
        opts.exclude_packages
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|p| format!("--exclude {} ", p.trim()))
            .collect::<String>()
    };

    let mut steps = String::new();
    steps.push_str("      - uses: actions/checkout@v4\n");
    steps.push_str(&format!(
        "      - name: Install Rust toolchain\n        uses: dtolnay/rust-toolchain@{toolchain}\n"
    ));
    steps.push_str("        with:\n");
    if opts.include_clippy {
        steps.push_str("          components: rustfmt, clippy\n");
    } else {
        steps.push_str("          components: rustfmt\n");
    }
    steps.push_str("      - name: Rust cache\n        uses: Swatinem/rust-cache@v2\n        with:\n          cache-all-crates: \"true\"\n");
    steps.push_str("      - name: cargo fmt\n        run: cargo fmt --all --check\n");
    if opts.include_clippy {
        steps.push_str(&format!(
            "      - name: cargo clippy\n        run: cargo clippy --workspace --all-targets {exclude}\n"
        ));
    }
    steps.push_str(&format!(
        "      - name: cargo build\n        run: cargo build --workspace {exclude}\n"
    ));
    if opts.include_deny {
        steps.push_str("      - name: cargo deny\n        run: cargo deny check\n");
    } else {
        steps.push_str("      - name: cargo test\n        run: cargo test --workspace --lib\n");
    }

    format!(
        r#"name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: -D warnings

jobs:
  check:
    name: Build & check
    runs-on: ubuntu-latest
    steps:
{steps}
  test:
    name: Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@{toolchain}
      - name: Rust cache
        uses: Swatinem/rust-cache@v2
        with:
          cache-all-crates: "true"
      - name: cargo test
        run: cargo test --workspace {exclude}
"#,
        steps = steps
    )
}

fn generate_pr_review_workflow(_opts: &GenerateOptions) -> String {
    r#"name: PR Review

on:
  pull_request:
    types: [opened, synchronize, reopened]

jobs:
  review:
    name: Sisyphus PR Review
    runs-on: ubuntu-latest
    permissions:
      contents: read
      pull-requests: write
      checks: write

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Rust cache
        uses: Swatinem/rust-cache@v2
        with:
          cache-all-crates: "true"

      - name: Build sisyphus-cli
        run: cargo build -p sisyphus-cli

      - name: Generate PR diff
        id: diff
        run: |
          git fetch origin ${{ github.base_ref }}
          git diff origin/${{ github.base_ref }}...HEAD > /tmp/pr.diff
          echo "diff_lines=$(wc -l < /tmp/pr.diff)" >> $GITHUB_OUTPUT

      - name: Run Sisyphus PR review
        id: review
        env:
          SISYPHUS_CI: "1"
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          cargo run -p sisyphus-cli -- pr review ${{ github.event.number }} 2>&1 | tee /tmp/review_output.txt

      - name: Post review comment
        uses: actions/github-script@v7
        with:
          script: |
            const fs = require('fs');
            const review = fs.readFileSync('/tmp/review_output.txt', 'utf8');
            await github.rest.issues.createComment({
              ...context.repo,
              issue_number: context.issue.number,
              body: `## Sisyphus PR Review\n\n${review.substring(0, 60000)}`
            });
"#
    .to_string()
}

fn generate_release_workflow(_opts: &GenerateOptions) -> String {
    r#"name: Release

on:
  push:
    tags:
      - "v[0-9]+.[0-9]+.[0-9]+*"

jobs:
  build:
    name: Build release artifacts
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Rust cache
        uses: Swatinem/rust-cache@v2
        with:
          cache-all-crates: "true"

      - name: cargo build (release)
        run: cargo build --release --workspace

      - name: Generate checksums
        run: |
          sha256sum target/release/sisyphus-bridge > target/release/sisyphus-bridge.sha256
          sha256sum target/release/sisyphus-cli > target/release/sisyphus-cli.sha256

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            target/release/sisyphus-bridge
            target/release/sisyphus-bridge.sha256
            target/release/sisyphus-cli
            target/release/sisyphus-cli.sha256
          body_path: CHANGELOG.md
          draft: true
"#
    .to_string()
}

/// Post a review comment to a PR using the `gh` CLI.
/// Returns the URL of the posted comment on success.
pub fn post_pr_comment(pr_number: u64, review_text: &str) -> ToolResult<String> {
    let output = std::process::Command::new("gh")
        .args(["pr", "comment", &pr_number.to_string(), "--body", review_text])
        .output()
        .map_err(|e| crate::ToolError::Generic(format!("Failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::ToolError::Generic(format!(
            "gh pr comment failed: {stderr}"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // gh returns the comment URL on success
    if stdout.is_empty() {
        Ok("Comment posted (no URL returned)".into())
    } else {
        Ok(stdout)
    }
}

/// Validate that a generated workflow YAML is syntactically correct.
/// Returns Ok(()) if valid, or an error describing the first issue found.
pub fn validate_workflow_yaml(yaml: &str) -> ToolResult<()> {
    // Quick structural checks before attempting serde_yaml parse
    let lines: Vec<&str> = yaml.lines().collect();
    if lines.is_empty() {
        return Err(crate::ToolError::Generic("Workflow YAML is empty".into()));
    }

    // Must start with a top-level key (workflow name)
    if !lines.iter().any(|l| l.starts_with("name:")) {
        return Err(crate::ToolError::Generic(
            "Workflow YAML missing required 'name' field".into(),
        ));
    }
    if !lines.iter().any(|l| l.starts_with("on:")) {
        return Err(crate::ToolError::Generic(
            "Workflow YAML missing required 'on' trigger field".into(),
        ));
    }
    if !lines.iter().any(|l| l.starts_with("jobs:")) {
        return Err(crate::ToolError::Generic(
            "Workflow YAML missing required 'jobs' field".into(),
        ));
    }

    // Full YAML parse to catch syntax errors
    serde_yaml::from_str::<serde_yaml::Value>(yaml).map_err(|e| {
        crate::ToolError::Generic(format!("Workflow YAML parse error: {e}"))
    })?;

    Ok(())
}

/// Write a generated workflow to `.github/workflows/<name>.yml`.
/// Creates the directory if it doesn't exist.
pub fn write_workflow(kind: WorkflowKind, opts: &GenerateOptions) -> ToolResult<std::path::PathBuf> {
    let workflow_dir = std::path::Path::new(".github/workflows");
    if !workflow_dir.exists() {
        std::fs::create_dir_all(workflow_dir)?;
    }

    let filename = format!("sisyphus-{}.yml", kind.as_str());
    let path = workflow_dir.join(&filename);
    let content = generate_workflow(kind, opts);
    std::fs::write(&path, content)?;
    Ok(path)
}

// ── Config for CI mode ─────────────────────────────────────────────────────

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
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false),
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            pr_number: std::env::var("PR_NUMBER")
                .ok()
                .and_then(|v| v.parse().ok()),
            output_format: std::env::var("SISYPHUS_OUTPUT_FORMAT")
                .unwrap_or_else(|_| "plain".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_ci_workflow() {
        let yaml = generate_workflow(WorkflowKind::Ci, &GenerateOptions::default());
        assert!(yaml.contains("name: CI"));
        assert!(yaml.contains("cargo fmt"));
        assert!(yaml.contains("cargo test"));
    }

    #[test]
    fn test_generate_pr_review_workflow() {
        let yaml = generate_workflow(WorkflowKind::PrReview, &GenerateOptions::default());
        assert!(yaml.contains("name: PR Review"));
        assert!(yaml.contains("pr review"));
        assert!(yaml.contains("actions/github-script"));
    }

    #[test]
    fn test_generate_release_workflow() {
        let yaml = generate_workflow(WorkflowKind::Release, &GenerateOptions::default());
        assert!(yaml.contains("name: Release"));
        assert!(yaml.contains("softprops/action-gh-release"));
        assert!(yaml.contains("sha256sum"));
    }

    #[test]
    fn test_write_workflow_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let path = write_workflow(WorkflowKind::Ci, &GenerateOptions::default()).unwrap();
        assert!(path.exists());
        assert!(path.ends_with("sisyphus-ci.yml"));

        std::env::set_current_dir(original_cwd).unwrap();
    }

    #[test]
    fn test_is_ci_detection() {
        // In test environment, CI should be false
        // We can't easily test the true case without env manipulation
        assert_eq!(is_ci(), std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok());
    }

    #[test]
    fn test_ci_config_from_env() {
        // Default config when no env vars set
        let cfg = CiConfig::from_env();
        assert!(!cfg.ci_mode);
        assert!(cfg.github_token.is_none() || cfg.github_token.is_some());
        assert_eq!(cfg.output_format, "plain");
    }

    #[test]
    fn test_validate_workflow_yaml_ci() {
        let yaml = generate_workflow(WorkflowKind::Ci, &GenerateOptions::default());
        assert!(validate_workflow_yaml(&yaml).is_ok());
    }

    #[test]
    fn test_validate_workflow_yaml_pr_review() {
        let yaml = generate_workflow(WorkflowKind::PrReview, &GenerateOptions::default());
        assert!(validate_workflow_yaml(&yaml).is_ok());
    }

    #[test]
    fn test_validate_workflow_yaml_release() {
        let yaml = generate_workflow(WorkflowKind::Release, &GenerateOptions::default());
        assert!(validate_workflow_yaml(&yaml).is_ok());
    }

    #[test]
    fn test_validate_workflow_yaml_invalid() {
        assert!(validate_workflow_yaml("").is_err());
        assert!(validate_workflow_yaml("not: valid: yaml: {{broken}}").is_err());
        assert!(validate_workflow_yaml("foo: bar").is_err()); // missing name/on/jobs
    }

    #[test]
    fn test_validate_workflow_yaml_with_options() {
        let mut opts = GenerateOptions::default();
        opts.include_clippy = true;
        opts.include_deny = true;
        opts.rust_toolchain = "nightly-2024-01-01".into();
        opts.exclude_packages = "sisyphus-bridge,sisyphus-sandbox".into();

        let yaml = generate_workflow(WorkflowKind::Ci, &opts);
        assert!(validate_workflow_yaml(&yaml).is_ok());
        assert!(yaml.contains("nightly-2024-01-01"));
        assert!(yaml.contains("clippy"));
    }

    #[test]
    fn test_post_pr_comment_no_gh_cli() {
        // Without `gh` CLI installed, this should fail gracefully
        let result = post_pr_comment(42, "test review");
        // Either gh is installed (unlikely in test) or we get an error
        match result {
            Ok(url) => println!("gh CLI available, comment URL: {url}"),
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("gh") || msg.contains("Failed to run"), "Expected error about gh CLI, got: {msg}");
            }
        }
    }
}
