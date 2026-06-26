use std::path::Path;
use std::process::Command;
use crate::{ToolError, ToolResult};

// ── Structured return types ────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub staged: Vec<FileStatus>,
    pub unstaged: Vec<FileStatus>,
    pub untracked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    pub path: String,
    /// One‑character status code: M modified, A added, D deleted, R renamed, ? untracked, etc.
    pub x: char, // index (staging area)
    pub y: char, // working tree
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitLog {
    pub entries: Vec<CommitEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub current: bool,
    pub name: String,
}

// ── Helpers ────────────────────────────────────────────────

fn git_args(args: &[&str], cwd: Option<&Path>) -> ToolResult<String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ToolError::Generic("git executable not found — is Git installed?".into())
        } else {
            ToolError::Io(e)
        }
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let code = output.status.code().unwrap_or(-1);
        return Err(ToolError::CommandFailed {
            code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn require_git_repo(cwd: Option<&Path>) -> ToolResult<()> {
    git_args(&["rev-parse", "--git-dir"], cwd)?;
    Ok(())
}

// ── Public API ─────────────────────────────────────────────

/// Return the current branch name.
pub fn current_branch(cwd: Option<&Path>) -> ToolResult<String> {
    let out = git_args(&["rev-parse", "--abbrev-ref", "HEAD"], cwd)?;
    if out.is_empty() || out == "HEAD" {
        // detached HEAD
        let hash = git_args(&["rev-parse", "--short", "HEAD"], cwd)?;
        Ok(format!("detached {}", hash))
    } else {
        Ok(out)
    }
}

/// Parse `git status --porcelain -b` into a structured status.
pub fn status(cwd: Option<&Path>) -> ToolResult<GitStatus> {
    require_git_repo(cwd)?;
    let out = git_args(&["status", "--porcelain", "-b"], cwd)?;

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    let mut branch = String::new();
    let mut ahead = 0usize;
    let mut behind = 0usize;

    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            // Handle "No commits yet on <branch>" for empty repos
            let branch_part = if let Some(nocommit) = rest.strip_prefix("No commits yet on ") {
                nocommit
            } else {
                rest
            };
            // "branch...upstream [ahead X, behind Y]" or just "branch"
            if let Some((br, _trailing)) = branch_part.split_once("...") {
                branch = br.to_string();
                // parse ahead/behind
                for token in _trailing.split(&[',', ' ', '\t'][..]) {
                    if let Some(val) = token.strip_prefix("ahead ") {
                        ahead = val.trim().parse().unwrap_or(0);
                    } else if let Some(val) = token.strip_prefix("behind ") {
                        behind = val.trim().parse().unwrap_or(0);
                    }
                }
            } else {
                branch = branch_part.to_string();
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let (xy, path) = line.split_at(2.min(line.len()));
        let xy = xy.trim().to_string();
        let p = path.trim().to_string();
        let x = xy.chars().next().unwrap_or(' ');
        let y = xy.chars().nth(1).unwrap_or(' ');

        let fs = FileStatus { path: p, x, y };

        if x == '?' && y == '?' {
            untracked.push(fs.path);
        } else if x != ' ' {
            staged.push(fs);
        } else {
            unstaged.push(fs);
        }
    }

    if branch.is_empty() {
        branch = current_branch(cwd)?;
    }

    Ok(GitStatus { branch, ahead, behind, staged, unstaged, untracked })
}

/// Run `git diff` and return the raw diff text.
pub fn diff(pathspec: Option<&str>, cached: bool, cwd: Option<&Path>) -> ToolResult<String> {
    require_git_repo(cwd)?;
    let mut args = vec!["diff"];
    if cached {
        args.push("--cached");
    }
    if let Some(p) = pathspec {
        args.push("--");
        args.push(p);
    }
    git_args(&args, cwd)
}

/// Stage files.  Pass `["."]` or `["--all"]` to stage everything.
pub fn add(pathspecs: &[&str], cwd: Option<&Path>) -> ToolResult<()> {
    require_git_repo(cwd)?;
    let mut args = vec!["add"];
    for p in pathspecs {
        args.push(p);
    }
    git_args(&args, cwd)?;
    Ok(())
}

/// Create a commit.
pub fn commit(message: &str, allow_empty: bool, cwd: Option<&Path>) -> ToolResult<String> {
    require_git_repo(cwd)?;
    let mut args = vec!["commit", "--message", message];
    if allow_empty {
        args.push("--allow-empty");
    }
    let out = git_args(&args, cwd)?;
    Ok(out)
}

/// Return structured commit log.
pub fn log(count: usize, cwd: Option<&Path>) -> ToolResult<GitCommitLog> {
    require_git_repo(cwd)?;
    let n = count.to_string();
    // format: hash%%author%%date%%message
    let out = git_args(&[
        "log",
        &format!("-{n}"),
        "--format=%h%%%an%%%ai%%%s",
    ], cwd)?;

    let mut entries = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.splitn(4, "%").collect();
        if parts.len() >= 4 {
            entries.push(CommitEntry {
                hash: parts[0].to_string(),
                author: parts[1].to_string(),
                date: parts[2].to_string(),
                message: parts[3].to_string(),
            });
        }
    }
    Ok(GitCommitLog { entries })
}

/// List local branches.
pub fn branch(cwd: Option<&Path>) -> ToolResult<Vec<BranchInfo>> {
    require_git_repo(cwd)?;
    let out = git_args(&["branch"], cwd)?;
    let mut branches = Vec::new();
    for line in out.lines() {
        let current = line.starts_with('*');
        let name = if current { &line[2..] } else { line.trim() };
        branches.push(BranchInfo { current, name: name.to_string() });
    }
    Ok(branches)
}

/// Checkout an existing branch, or create‑and‑switch with `-b`.
pub fn checkout(branch: &str, create: bool, cwd: Option<&Path>) -> ToolResult<String> {
    require_git_repo(cwd)?;
    let mut args = vec!["checkout"];
    if create {
        args.push("-b");
    }
    args.push(branch);
    git_args(&args, cwd)
}

/// Push commits to remote.
pub fn push(remote: &str, branch: &str, force: bool, cwd: Option<&Path>) -> ToolResult<String> {
    require_git_repo(cwd)?;
    let mut args = vec!["push", remote, branch];
    if force {
        args.push("--force");
    }
    git_args(&args, cwd)
}

/// Pull from remote.
pub fn pull(remote: &str, branch: &str, cwd: Option<&Path>) -> ToolResult<String> {
    require_git_repo(cwd)?;
    git_args(&["pull", remote, branch], cwd)
}

/// Initialise a new git repository.
pub fn init(path: &Path) -> ToolResult<String> {
    let out = git_args(&["init"], Some(path))?;
    Ok(out)
}

/// Return the diff of HEAD~1..HEAD (last commit diff).
pub fn last_commit_diff(cwd: Option<&Path>) -> ToolResult<String> {
    require_git_repo(cwd)?;
    git_args(&["diff", "HEAD~1..HEAD"], cwd)
}

/// Return a short one‑line summary like `"main +3 ~2"`.
pub fn summary(cwd: Option<&Path>) -> ToolResult<String> {
    let s = status(cwd)?;
    let staged_count = s.staged.len();
    let unstaged_count = s.unstaged.len();
    let untracked_count = s.untracked.len();
    Ok(format!(
        "{} +{} ~{} ?{}",
        s.branch,
        staged_count,
        unstaged_count,
        untracked_count,
    ))
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::fs;
    use tempfile::TempDir;

    fn init_repo() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        init(&path).unwrap();
        git_args(&["config", "user.email", "test@test.com"], Some(&path)).unwrap();
        git_args(&["config", "user.name", "Test User"], Some(&path)).unwrap();
        (dir, path)
    }

    fn write_file(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[allow(dead_code)]
    fn git(args: &[&str], dir: &Path) -> String {
        git_args(args, Some(dir)).unwrap()
    }

    #[test]
    fn test_init_and_status_empty() {
        let (_d, path) = init_repo();
        let s = status(Some(&path)).unwrap();
        assert_eq!(s.branch, "master");
        assert!(s.staged.is_empty());
        assert!(s.unstaged.is_empty());
        assert!(s.untracked.is_empty());
    }

    #[test]
    fn test_status_untracked() {
        let (_d, path) = init_repo();
        write_file(&path, "hello.txt", "hello world");
        let s = status(Some(&path)).unwrap();
        assert_eq!(s.untracked, vec!["hello.txt"]);
    }

    #[test]
    fn test_add_and_commit() {
        let (_d, path) = init_repo();
        write_file(&path, "a.txt", "aaa");
        add(&["."], Some(&path)).unwrap();
        let s1 = status(Some(&path)).unwrap();
        assert!(!s1.staged.is_empty());
        assert_eq!(s1.untracked.len(), 0); // staged

        commit("initial", false, Some(&path)).unwrap();
        let s2 = status(Some(&path)).unwrap();
        assert!(s2.staged.is_empty());
        assert!(s2.unstaged.is_empty());
        assert!(s2.untracked.is_empty());
    }

    #[test]
    fn test_log() {
        let (_d, path) = init_repo();
        write_file(&path, "f1", "one");
        add(&["."], Some(&path)).unwrap();
        commit("first", false, Some(&path)).unwrap();

        write_file(&path, "f2", "two");
        add(&["."], Some(&path)).unwrap();
        commit("second", false, Some(&path)).unwrap();

        let log = log(10, Some(&path)).unwrap();
        assert_eq!(log.entries.len(), 2);
        assert_eq!(log.entries[0].message, "second");
        assert_eq!(log.entries[1].message, "first");
    }

    #[test]
    fn test_branch_and_checkout() {
        let (_d, path) = init_repo();
        write_file(&path, "x", "x");
        add(&["."], Some(&path)).unwrap();
        commit("init", false, Some(&path)).unwrap();

        let branches = branch(Some(&path)).unwrap();
        let current: Vec<&BranchInfo> = branches.iter().filter(|b| b.current).collect();
        assert_eq!(current.len(), 1);

        let _result = checkout("feature", true, Some(&path)).unwrap();
        // git writes switch messages to stderr, so stdout may be empty
        assert_eq!(current_branch(Some(&path)).unwrap(), "feature");
    }

    #[test]
    fn test_diff() {
        let (_d, path) = init_repo();
        write_file(&path, "d.txt", "hello\nworld\n");
        add(&["."], Some(&path)).unwrap();
        commit("init", false, Some(&path)).unwrap();

        write_file(&path, "d.txt", "hello\nmodified\nworld\n");
        let d = diff(None, false, Some(&path)).unwrap();
        assert!(d.contains("+modified"));
    }

    #[test]
    fn test_summary() {
        let (_d, path) = init_repo();
        let s = summary(Some(&path)).unwrap();
        // something like "master +0 ~0 ?0"
        assert!(s.contains('+'));
        assert!(s.contains('~'));
        assert!(s.contains('?'));
    }

    #[test]
    fn test_pull_push_fail_no_remote() {
        let (_d, path) = init_repo();
        write_file(&path, "p", "p");
        add(&["."], Some(&path)).unwrap();
        commit("init", false, Some(&path)).unwrap();

        // push with no remote should fail
        let err = push("origin", "master", false, Some(&path)).unwrap_err();
        match err {
            ToolError::CommandFailed { .. } => {} // expected
            _ => panic!("expected CommandFailed, got {err:?}"),
        }
    }

    #[test]
    fn test_current_branch_after_checkout() {
        let (_d, path) = init_repo();
        write_file(&path, "m", "main");
        add(&["."], Some(&path)).unwrap();
        commit("main", false, Some(&path)).unwrap();

        checkout("dev", true, Some(&path)).unwrap();
        assert_eq!(current_branch(Some(&path)).unwrap(), "dev");

        checkout("master", false, Some(&path)).unwrap();
        assert_eq!(current_branch(Some(&path)).unwrap(), "master");
    }

    #[test]
    fn test_status_ahead_behind() {
        // We can't test ahead/behind easily without a remote, but we can
        // confirm the parse doesn't crash for a simple branch line.
        let (_d, path) = init_repo();
        let s = status(Some(&path)).unwrap();
        assert!(!s.branch.is_empty());
    }
}
