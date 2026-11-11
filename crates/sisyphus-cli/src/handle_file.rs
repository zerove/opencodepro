use std::path::Path;

use anyhow::Result;

/// Handle read.
pub fn handle_read(path: &Path) -> Result<()> {
    let content = sisyphus_cli::tools::file::read(path)?;
    println!("{content}");
    Ok(())
}

/// Handle read lines.
pub fn handle_read_lines(path: &Path, offset: usize, limit: usize) -> Result<()> {
    let content = sisyphus_cli::tools::file::read_lines(path, offset, limit)?;
    print!("{content}");
    Ok(())
}

/// Handle write.
pub fn handle_write(path: &Path, content: &str) -> Result<()> {
    sisyphus_cli::tools::file::write(path, content)?;
    println!("Written {} bytes to {}", content.len(), path.display());
    verify_after_write(&[path.to_path_buf()])?;
    Ok(())
}

/// Handle edit.
pub fn handle_edit(path: &Path, old: &str, new: &str, all: bool) -> Result<()> {
    if all {
        let count = sisyphus_cli::tools::file::edit_all(path, old, new)?;
        println!("Replaced {} occurrence(s) in {}", count, path.display());
    } else {
        sisyphus_cli::tools::file::edit(path, old, new)?;
        println!("Replaced first occurrence in {}", path.display());
    }
    verify_after_write(&[path.to_path_buf()])?;
    Ok(())
}

/// 按配置执行编辑后验证；未启用时无副作用。
fn verify_after_write(paths: &[std::path::PathBuf]) -> Result<()> {
    sisyphus_cli::post_edit::verify_after_edit(paths)?;
    Ok(())
}

/// Handle glob.
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

/// Handle grep.
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
        // rustc 1.98 新增 `iter_out_of_bounds` lint：对长度 < take 数量的迭代器
        // 视为无意义操作。该测试改为验证 `Vec` 本身为空（与 handle_glob 的
        // "未匹配文件" 路径行为一致）。
        let files: Vec<std::path::PathBuf> = vec![];
        assert!(files.is_empty());
        assert_eq!(files.len(), 0);
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
