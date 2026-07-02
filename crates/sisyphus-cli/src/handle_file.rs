use std::path::Path;

use anyhow::Result;

pub fn handle_read(path: &Path) -> Result<()> {
    let content = sisyphus_tools::file::read(path)?;
    println!("{content}");
    Ok(())
}

pub fn handle_read_lines(path: &Path, offset: usize, limit: usize) -> Result<()> {
    let content = sisyphus_tools::file::read_lines(path, offset, limit)?;
    print!("{content}");
    Ok(())
}

pub fn handle_write(path: &Path, content: &str) -> Result<()> {
    sisyphus_tools::file::write(path, content)?;
    println!("Written {} bytes to {}", content.len(), path.display());
    Ok(())
}

pub fn handle_edit(path: &Path, old: &str, new: &str, all: bool) -> Result<()> {
    if all {
        let count = sisyphus_tools::file::edit_all(path, old, new)?;
        println!("Replaced {} occurrence(s) in {}", count, path.display());
    } else {
        sisyphus_tools::file::edit(path, old, new)?;
        println!("Replaced first occurrence in {}", path.display());
    }
    Ok(())
}

pub fn handle_glob(pattern: &str, root: &Path, limit: usize) -> Result<()> {
    let files = sisyphus_tools::file::glob(pattern, root)?;
    let shown = files.iter().take(limit);
    for f in shown {
        println!("{}", f.display());
    }
    if files.len() > limit {
        println!("... and {} more", files.len() - limit);
    } else if files.is_empty() {
        println!("No files matched: {pattern}");
    }
    Ok(())
}

pub fn handle_grep(pattern: &str, path: &Path, include: Option<&str>) -> Result<()> {
    let matches = sisyphus_tools::file::grep(pattern, path, include)?;
    for m in &matches {
        println!("{}:{} — {}", m.file.display(), m.line, m.content.trim());
    }
    if matches.is_empty() {
        println!("No matches for pattern: {pattern}");
    }
    Ok(())
}
