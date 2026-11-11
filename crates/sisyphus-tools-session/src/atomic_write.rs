//! 原子写文件。
//!
//! 直接 `fs::write` 是「截断 → 写入」，并发读者可能读到**半截内容**。
//! 对 JSON 存储而言，半截内容会解析失败；若加载方对解析失败静默回退到空结构，
//! 就会表现为「刚写入的数据凭空消失」（例如并行测试下 todo 列表忽空忽有）。
//!
//! 本模块实现「写同目录临时文件 → `rename` 覆盖」的原子替换：
//! 读者只能看到替换前的完整内容或替换后的完整内容，不会看到中间态。

use std::fs;
use std::io;
use std::path::Path;

/// 原子写入文件内容。
///
/// 临时文件名带进程 id 与纳秒时间戳，避免同进程内并发写互相覆盖。
pub fn write_atomic(path: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let mut tmp_name = path.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    tmp_name.push(format!(".tmp-{}-{stamp}", std::process::id()));
    let tmp_path = path.with_file_name(tmp_name);

    fs::write(&tmp_path, content)?;
    if let Err(err) = fs::rename(&tmp_path, path) {
        // 覆盖失败时清理临时文件，避免留下垃圾文件
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_content_and_leaves_no_temp_files() {
        let dir =
            std::env::temp_dir().join(format!("sisyphus-atomic-write-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("data.json");

        write_atomic(&target, b"{\"a\":1}").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"a\":1}");

        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replaces_existing_content() {
        let dir = std::env::temp_dir().join(format!(
            "sisyphus-atomic-write-replace-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("data.json");

        write_atomic(&target, b"old").unwrap();
        write_atomic(&target, b"new").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");

        let _ = fs::remove_dir_all(&dir);
    }
}
