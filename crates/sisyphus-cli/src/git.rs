use anyhow::Result;
use std::path::PathBuf;

/// 解析 `cwd` 参数，默认使用当前目录。
fn resolve_dir(cwd: Option<PathBuf>) -> Option<PathBuf> {
    Some(cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_default()))
}

// ── 状态 ─────────────────────────────────────────────────

/// Run status.
pub fn run_status(cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let s = crate::tools::git::status(dir.as_deref())?;

    println!("On branch: {}", s.branch);
    if s.ahead > 0 || s.behind > 0 {
        println!(
            "  ahead {} · behind {} relative to upstream",
            s.ahead, s.behind
        );
    }

    if !s.staged.is_empty() {
        println!("\nStaged changes:");
        for f in &s.staged {
            println!("  {}  {}", status_char(f.x), f.path);
        }
    }
    if !s.unstaged.is_empty() {
        println!("\nUnstaged changes:");
        for f in &s.unstaged {
            println!("  {}  {}", status_char(f.y), f.path);
        }
    }
    if !s.untracked.is_empty() {
        println!("\nUntracked files:");
        for f in &s.untracked {
            println!("  ?  {f}");
        }
    }

    if s.staged.is_empty() && s.unstaged.is_empty() && s.untracked.is_empty() {
        println!("Working tree clean.");
    }
    Ok(())
}

fn status_char(c: char) -> &'static str {
    match c {
        'M' => "M",
        'A' => "A",
        'D' => "D",
        'R' => "R",
        'C' => "C",
        'U' => "U",
        '?' => "?",
        '!' => "!",
        _ => "\u{b7}",
    }
}

// ── 差异 ───────────────────────────────────────────────────

/// Run diff.
pub fn run_diff(pathspec: Option<String>, cached: bool, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let d = crate::tools::git::diff(pathspec.as_deref(), cached, dir.as_deref())?;
    if d.is_empty() {
        println!("No diff.");
    } else {
        println!("{d}");
    }
    Ok(())
}

// ── 添加 ────────────────────────────────────────────────────

/// Run add.
pub fn run_add(pathspecs: Vec<String>, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let refs: Vec<&str> = pathspecs.iter().map(|s| s.as_str()).collect();
    if refs.is_empty() {
        anyhow::bail!("at least one path required (use '.' to stage everything)");
    }
    crate::tools::git::add(&refs, dir.as_deref())?;
    println!("Staged {} path(s).", refs.len());
    Ok(())
}

// ── 提交 ─────────────────────────────────────────────────

/// Run commit.
pub fn run_commit(message: String, allow_empty: bool, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let out = crate::tools::git::commit(&message, allow_empty, dir.as_deref())?;
    println!("{out}");
    Ok(())
}

// ── 日志 ────────────────────────────────────────────────────

/// Run log.
pub fn run_log(count: usize, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let log = crate::tools::git::log(count, dir.as_deref())?;
    if log.entries.is_empty() {
        println!("No commits.");
        return Ok(());
    }
    for e in &log.entries {
        println!(
            "{}  {}  {}  {}",
            &e.hash[..7.min(e.hash.len())],
            e.author,
            &e.date[..10.min(e.date.len())],
            e.message
        );
    }
    Ok(())
}

// ── 分支 ─────────────────────────────────────────────────

/// Run branch.
pub fn run_branch(cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let branches = crate::tools::git::branch(dir.as_deref())?;
    for b in &branches {
        let mark = if b.current { "* " } else { "  " };
        println!("{mark}{}", b.name);
    }
    Ok(())
}

// ── 检出 ───────────────────────────────────────────────

/// Run checkout.
pub fn run_checkout(branch: String, create: bool, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let out = crate::tools::git::checkout(&branch, create, dir.as_deref())?;
    println!("{out}");
    Ok(())
}

// ── 推送 ───────────────────────────────────────────────────

/// Run push.
pub fn run_push(
    remote: String,
    branch: Option<String>,
    force: bool,
    cwd: Option<PathBuf>,
) -> Result<()> {
    let dir = resolve_dir(cwd);
    let br = branch.unwrap_or_else(|| {
        crate::tools::git::current_branch(dir.as_deref()).unwrap_or_else(|_| "main".into())
    });
    let out = crate::tools::git::push(&remote, &br, force, dir.as_deref())?;
    println!("{out}");
    Ok(())
}

// ── 拉取 ───────────────────────────────────────────────────

/// Run pull.
pub fn run_pull(remote: String, branch: Option<String>, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let br = branch.unwrap_or_else(|| {
        crate::tools::git::current_branch(dir.as_deref()).unwrap_or_else(|_| "main".into())
    });
    let out = crate::tools::git::pull(&remote, &br, dir.as_deref())?;
    println!("{out}");
    Ok(())
}

// ── 摘要 ────────────────────────────────────────────────

/// Run summary.
pub fn run_summary(cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let s = crate::tools::git::summary(dir.as_deref())?;
    println!("{s}");
    Ok(())
}
