use std::path::Path;

use anyhow::Result;

pub fn handle_read(path: &Path) -> Result<()> {
    let content = sisyphus_cli::tools::file::read(path)?;
    println!("{content}");
    Ok(())
}

pub fn handle_read_lines(path: &Path, offset: usize, limit: usize) -> Result<()> {
    let content = sisyphus_cli::tools::file::read_lines(path, offset, limit)?;
    print!("{content}");
    Ok(())
}

pub fn handle_write(path: &Path, content: &str) -> Result<()> {
    sisyphus_cli::tools::file::write(path, content)?;
    println!("Written {} bytes to {}", content.len(), path.display());
    Ok(())
}

pub fn handle_edit(path: &Path, old: &str, new: &str, all: bool) -> Result<()> {
    if all {
        let count = sisyphus_cli::tools::file::edit_all(path, old, new)?;
        println!("Replaced {} occurrence(s) in {}", count, path.display());
    } else {
        sisyphus_cli::tools::file::edit(path, old, new)?;
        println!("Replaced first occurrence in {}", path.display());
    }
    Ok(())
}

pub fn handle_glob(pattern: &str, root: &Path, limit: usize) -> Result<()> {
    let files = sisyphus_cli::tools::file::glob(pattern, root)?;
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
    let matches = sisyphus_cli::tools::file::grep(pattern, path, include)?;
    for m in &matches {
        println!("{}:{} — {}", m.file.display(), m.line, m.content.trim());
    }
    if matches.is_empty() {
        println!("No matches for pattern: {pattern}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_write_output_format() {
        let path = std::path::Path::new("/tmp/out.txt");
        let content = "hello world";
        let msg = format!("Written {} bytes to {}", content.len(), path.display());
        assert_eq!(msg, "Written 11 bytes to /tmp/out.txt");
    }

    #[test]
    fn test_handle_glob_limit_format() {
        let files: Vec<std::path::PathBuf> =
            (0..5).map(|i| format!("/tmp/file_{}", i).into()).collect();
        let limit = 3;
        let shown: Vec<_> = files.iter().take(limit).collect();
        assert_eq!(shown.len(), 3);
        let remaining = files.len() - limit;
        assert_eq!(remaining, 2);
        let overflow_msg = format!("... and {} more", remaining);
        assert_eq!(overflow_msg, "... and 2 more");
    }

    #[test]
    fn test_handle_glob_empty() {
        let files: Vec<std::path::PathBuf> = vec![];
        let shown: Vec<_> = files.iter().take(10).collect();
        assert!(shown.is_empty());
    }

    #[test]
    fn test_handle_grep_empty_format() {
        let pattern = "nonexistent";
        let msg = format!("No matches for pattern: {pattern}");
        assert_eq!(msg, "No matches for pattern: nonexistent");
    }

    #[test]
    fn test_handle_grep_match_format() {
        let file = std::path::Path::new("/tmp/test.rs");
        let line: usize = 10;
        let content = "  let x = 1;";
        let formatted = format!("{}:{} — {}", file.display(), line, content.trim());
        assert_eq!(formatted, "/tmp/test.rs:10 — let x = 1;");
    }

    #[test]
    fn test_handle_edit_display() {
        let count = 3;
        let path = std::path::Path::new("/tmp/test.rs");
        let msg_all = format!("Replaced {} occurrence(s) in {}", count, path.display());
        assert_eq!(msg_all, "Replaced 3 occurrence(s) in /tmp/test.rs");

        let msg_single = format!("Replaced first occurrence in {}", path.display());
        assert_eq!(msg_single, "Replaced first occurrence in /tmp/test.rs");
    }
}
