use anyhow::Result;
use std::path::PathBuf;

/// Resolve `cwd` parameter, defaulting to current directory.
fn resolve_dir(cwd: Option<PathBuf>) -> Option<PathBuf> {
    Some(cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_default()))
}

// ── status ─────────────────────────────────────────────────

pub async fn run_status(cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let s = sisyphus_tools::git::status(dir.as_deref())?;

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

// ── diff ───────────────────────────────────────────────────

pub async fn run_diff(pathspec: Option<String>, cached: bool, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let d = sisyphus_tools::git::diff(pathspec.as_deref(), cached, dir.as_deref())?;
    if d.is_empty() {
        println!("No diff.");
    } else {
        println!("{d}");
    }
    Ok(())
}

// ── add ────────────────────────────────────────────────────

pub async fn run_add(pathspecs: Vec<String>, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let refs: Vec<&str> = pathspecs.iter().map(|s| s.as_str()).collect();
    if refs.is_empty() {
        anyhow::bail!("at least one path required (use '.' to stage everything)");
    }
    sisyphus_tools::git::add(&refs, dir.as_deref())?;
    println!("Staged {} path(s).", refs.len());
    Ok(())
}

// ── commit ─────────────────────────────────────────────────

pub async fn run_commit(message: String, allow_empty: bool, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let out = sisyphus_tools::git::commit(&message, allow_empty, dir.as_deref())?;
    println!("{out}");
    Ok(())
}

// ── log ────────────────────────────────────────────────────

pub async fn run_log(count: usize, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let log = sisyphus_tools::git::log(count, dir.as_deref())?;
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

// ── branch ─────────────────────────────────────────────────

pub async fn run_branch(cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let branches = sisyphus_tools::git::branch(dir.as_deref())?;
    for b in &branches {
        let mark = if b.current { "* " } else { "  " };
        println!("{mark}{}", b.name);
    }
    Ok(())
}

// ── checkout ───────────────────────────────────────────────

pub async fn run_checkout(branch: String, create: bool, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let out = sisyphus_tools::git::checkout(&branch, create, dir.as_deref())?;
    println!("{out}");
    Ok(())
}

// ── push ───────────────────────────────────────────────────

pub async fn run_push(
    remote: String,
    branch: Option<String>,
    force: bool,
    cwd: Option<PathBuf>,
) -> Result<()> {
    let dir = resolve_dir(cwd);
    let br = branch.unwrap_or_else(|| {
        sisyphus_tools::git::current_branch(dir.as_deref()).unwrap_or_else(|_| "main".into())
    });
    let out = sisyphus_tools::git::push(&remote, &br, force, dir.as_deref())?;
    println!("{out}");
    Ok(())
}

// ── pull ───────────────────────────────────────────────────

pub async fn run_pull(remote: String, branch: Option<String>, cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let br = branch.unwrap_or_else(|| {
        sisyphus_tools::git::current_branch(dir.as_deref()).unwrap_or_else(|_| "main".into())
    });
    let out = sisyphus_tools::git::pull(&remote, &br, dir.as_deref())?;
    println!("{out}");
    Ok(())
}

// ── summary ────────────────────────────────────────────────

pub async fn run_summary(cwd: Option<PathBuf>) -> Result<()> {
    let dir = resolve_dir(cwd);
    let s = sisyphus_tools::git::summary(dir.as_deref())?;
    println!("{s}");
    Ok(())
}
