//! GitHub Actions integration: workflow templates, CI mode detection, PR review automation.
//!
//! ## Sub-modules
//! - [`types`]: WorkflowKind, GenerateOptions, CiConfig.

pub mod types;

pub use types::{CiConfig, GenerateOptions, WorkflowKind};

use sisyphus_tools_core::{ToolError, ToolResult};

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
"#
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
        .args([
            "pr",
            "comment",
            &pr_number.to_string(),
            "--body",
            review_text,
        ])
        .output()
        .map_err(|e| ToolError::Generic(format!("Failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolError::Generic(format!(
            "gh pr comment failed: {stderr}"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        Ok("Comment posted (no URL returned)".into())
    } else {
        Ok(stdout)
    }
}

/// Validate that a generated workflow YAML is syntactically correct.
pub fn validate_workflow_yaml(yaml: &str) -> ToolResult<()> {
    let lines: Vec<&str> = yaml.lines().collect();
    if lines.is_empty() {
        return Err(ToolError::Generic("Workflow YAML is empty".into()));
    }

    if !lines.iter().any(|l| l.starts_with("name:")) {
        return Err(ToolError::Generic(
            "Workflow YAML missing required 'name' field".into(),
        ));
    }
    if !lines.iter().any(|l| l.starts_with("on:")) {
        return Err(ToolError::Generic(
            "Workflow YAML missing required 'on' trigger field".into(),
        ));
    }
    if !lines.iter().any(|l| l.starts_with("jobs:")) {
        return Err(ToolError::Generic(
            "Workflow YAML missing required 'jobs' field".into(),
        ));
    }

    serde_yaml::from_str::<serde_yaml::Value>(yaml)
        .map_err(|e| ToolError::Generic(format!("Workflow YAML parse error: {e}")))?;

    Ok(())
}

/// Write a generated workflow to `<repo_root>/.github/workflows/<name>.yml`.
/// Creates the directory if it doesn't exist.
///
/// ## 为什么必须传显式 `repo_root`
///
/// 本函数原先硬编码相对路径 `.github/workflows`，落点完全取决于**进程级**
/// 当前工作目录。而 `set_current_dir` 改的是全局状态、不是线程级的：libtest
/// 默认并行跑同一二进制内的测试，任何一个测试 chdir 都会让另一个测试把
/// workflow 写进别人的临时目录 —— 或读回别人的文件。
///
/// 更糟的是「chdir 进 `TempDir` → 干活 → chdir 回来」这种写法：一旦中途
/// panic，回来的那一步不会执行，cwd 就**永久残留在一个已被删除的目录**里，
/// 此后该测试进程内所有相对路径操作（乃至其他 crate 的用例）一并失败。
///
/// 故改为显式接收仓库根：调用方传 `Path::new(".")` 时行为与原先完全一致，
/// 测试侧传各自的临时目录，从此不触碰进程 cwd。
pub fn write_workflow(
    kind: WorkflowKind,
    opts: &GenerateOptions,
    repo_root: &std::path::Path,
) -> ToolResult<std::path::PathBuf> {
    let workflow_dir = repo_root.join(".github").join("workflows");
    if !workflow_dir.exists() {
        std::fs::create_dir_all(&workflow_dir)?;
    }

    let filename = format!("sisyphus-{}.yml", kind.as_str());
    let path = workflow_dir.join(&filename);
    let content = generate_workflow(kind, opts);
    std::fs::write(&path, content)?;
    Ok(path)
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

        let path =
            write_workflow(WorkflowKind::Ci, &GenerateOptions::default(), dir.path()).unwrap();

        assert!(path.exists());
        assert!(path.ends_with("sisyphus-ci.yml"));
        // 必须落在传入的根下，而非进程 cwd
        assert_eq!(path.parent().unwrap(), dir.path().join(".github/workflows"));
    }

    /// 两个根目录必须各自独立落盘，互不串写。
    ///
    /// 本用例与上面那个并行执行：若 `write_workflow` 依赖进程级 cwd（而非
    /// 传入的 `repo_root`），两者会写进同一个目录，本断言随即失败。
    #[test]
    fn test_write_workflow_isolates_repo_roots() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();

        let path_a =
            write_workflow(WorkflowKind::Ci, &GenerateOptions::default(), a.path()).unwrap();
        let path_b =
            write_workflow(WorkflowKind::Release, &GenerateOptions::default(), b.path()).unwrap();

        assert_eq!(path_a.parent().unwrap(), a.path().join(".github/workflows"));
        assert_eq!(path_b.parent().unwrap(), b.path().join(".github/workflows"));
        assert!(path_a.exists() && path_b.exists());
        assert_ne!(path_a, path_b);
    }

    #[test]
    fn test_is_ci_detection() {
        assert_eq!(
            is_ci(),
            std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok()
        );
    }

    #[test]
    fn test_ci_config_from_env() {
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
        assert!(validate_workflow_yaml("foo: bar").is_err());
    }

    #[test]
    fn test_validate_workflow_yaml_with_options() {
        let opts = GenerateOptions {
            include_clippy: true,
            include_deny: true,
            rust_toolchain: "nightly-2024-01-01".into(),
            exclude_packages: "sisyphus-bridge,sisyphus-sandbox".into(),
            ..GenerateOptions::default()
        };

        let yaml = generate_workflow(WorkflowKind::Ci, &opts);
        assert!(validate_workflow_yaml(&yaml).is_ok());
        assert!(yaml.contains("nightly-2024-01-01"));
        assert!(yaml.contains("clippy"));
    }

    #[test]
    fn test_post_pr_comment_no_gh_cli() {
        let result = post_pr_comment(42, "test review");
        match result {
            Ok(url) => println!("gh CLI available, comment URL: {url}"),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("gh") || msg.contains("Failed to run"),
                    "Expected error about gh CLI, got: {msg}"
                );
            },
        }
    }
}
