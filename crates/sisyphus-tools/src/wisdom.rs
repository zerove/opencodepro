use crate::{ToolError, ToolResult};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Manager for cross-session knowledge retention (Wisdom/Notepads).
pub struct NotepadManager {
    base_path: PathBuf,
}

impl NotepadManager {
    /// Create a new NotepadManager with the given base path.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            base_path: path.as_ref().to_path_buf(),
        }
    }

    /// Create a NotepadManager with the default base path (.omo/notepads).
    pub fn default_dir() -> Self {
        Self::new(".omo/notepads")
    }

    async fn get_file_path(&self, category: &str) -> PathBuf {
        self.base_path.join(format!("{}.md", category))
    }

    /// Appends a new entry to the specified category notepad.
    pub async fn record(&self, category: &str, content: &str) -> ToolResult<()> {
        if !self.base_path.exists() {
            fs::create_dir_all(&self.base_path).await?;
        }

        let path = self.get_file_path(category).await;
        let timestamp = now_timestamp();
        let entry = format!("\n## {}\n{}\n", timestamp, content);

        if !path.exists() {
            let header = format!("# {}\n> Knowledge accumulation for {category}\n\n", category);
            let mut file = fs::File::create(&path).await?;
            file.write_all(header.as_bytes()).await?;
            file.write_all(entry.as_bytes()).await?;
        } else {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .await?;
            file.write_all(entry.as_bytes()).await?;
        }

        Ok(())
    }

    /// Lists all available notepad categories.
    pub async fn list(&self) -> ToolResult<Vec<String>> {
        if !self.base_path.exists() {
            return Ok(Vec::new());
        }

        let mut categories = Vec::new();
        let mut entries = fs::read_dir(&self.base_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    categories.push(stem.to_string());
                }
            }
        }

        categories.sort();
        Ok(categories)
    }

    /// Retrieves the content of a specific category notepad.
    pub async fn get(&self, category: &str) -> ToolResult<String> {
        let path = self.get_file_path(category).await;
        if !path.exists() {
            return Err(ToolError::NotFound(format!("notepad category not found: {category}")));
        }
        fs::read_to_string(path).await.map_err(ToolError::Io)
    }

    /// Deletes a specific category notepad.
    pub async fn delete(&self, category: &str) -> ToolResult<()> {
        let path = self.get_file_path(category).await;
        if !path.exists() {
            return Err(ToolError::NotFound(format!("notepad category not found: {category}")));
        }
        fs::remove_file(path).await.map_err(ToolError::Io)
    }

    /// Formats all notepads into a context block for prompt injection.
    pub async fn inject(&self) -> ToolResult<String> {
        let categories = self.list().await?;
        if categories.is_empty() {
            return Ok(String::new());
        }

        let mut wisdom_block = String::from("<wisdom>\n");
        for category in categories {
            if let Ok(content) = self.get(&category).await {
                wisdom_block.push_str(&format!("## {}\n{}\n\n", category, content));
            }
        }
        wisdom_block.push_str("</wisdom>");

        Ok(wisdom_block)
    }
}

/// Helper to generate a timestamp in YYYY-MM-DD HH:MM format.
fn now_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let secs_in_day = secs % 86400;
    let hours = secs_in_day / 3600;
    let minutes = (secs_in_day % 3600) / 60;
    let days_since_epoch = secs / 86400;
    let year = 1970 + (days_since_epoch as f64 / 365.25) as u64;
    let month = 1 + ((days_since_epoch % 365) as f64 / 30.44) as u64;
    let day = 1 + ((days_since_epoch % 365) as f64 % 30.44) as u64;
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}")
}

// --- Public API ---

pub async fn wisdom_record(category: &str, content: &str) -> ToolResult<()> {
    NotepadManager::default_dir().record(category, content).await
}

pub async fn wisdom_list() -> ToolResult<Vec<String>> {
    NotepadManager::default_dir().list().await
}

pub async fn wisdom_get(category: &str) -> ToolResult<String> {
    NotepadManager::default_dir().get(category).await
}

pub async fn wisdom_delete(category: &str) -> ToolResult<()> {
    NotepadManager::default_dir().delete(category).await
}

pub async fn wisdom_inject() -> ToolResult<String> {
    NotepadManager::default_dir().inject().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_record_and_get() {
        let tmp = TempDir::new().expect("create temp dir");
        let manager = NotepadManager::new(tmp.path());

        manager.record("conventions", "Use async/await").await.expect("record should succeed");
        let content = manager.get("conventions").await.expect("get should succeed");
        
        assert!(content.contains("# conventions"));
        assert!(content.contains("Use async/await"));
    }

    #[tokio::test]
    async fn test_record_appends() {
        let tmp = TempDir::new().expect("create temp dir");
        let manager = NotepadManager::new(tmp.path());

        manager.record("experience", "First entry").await.expect("record 1 should succeed");
        manager.record("experience", "Second entry").await.expect("record 2 should succeed");
        
        let content = manager.get("experience").await.expect("get should succeed");
        assert!(content.contains("First entry"));
        assert!(content.contains("Second entry"));
    }

    #[tokio::test]
    async fn test_list_categories() {
        let tmp = TempDir::new().expect("create temp dir");
        let manager = NotepadManager::new(tmp.path());

        manager.record("cat1", "content1").await.expect("record 1 should succeed");
        manager.record("cat2", "content2").await.expect("record 2 should succeed");
        
        let list = manager.list().await.expect("list should succeed");
        assert_eq!(list, vec!["cat1", "cat2"]);
    }

    #[tokio::test]
    async fn test_delete_category() {
        let tmp = TempDir::new().expect("create temp dir");
        let manager = NotepadManager::new(tmp.path());

        manager.record("temp", "content").await.expect("record should succeed");
        manager.delete("temp").await.expect("delete should succeed");
        
        let result = manager.get("temp").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_inject_context() {
        let tmp = TempDir::new().expect("create temp dir");
        let manager = NotepadManager::new(tmp.path());

        manager.record("conventions", "Rule 1").await.expect("record 1 should succeed");
        manager.record("successes", "Win 1").await.expect("record 2 should succeed");
        
        let injected = manager.inject().await.expect("inject should succeed");
        assert!(injected.starts_with("<wisdom>"));
        assert!(injected.ends_with("</wisdom>"));
        assert!(injected.contains("## conventions"));
        assert!(injected.contains("Rule 1"));
        assert!(injected.contains("## successes"));
        assert!(injected.contains("Win 1"));
    }

    #[tokio::test]
    async fn test_inject_empty() {
        let tmp = TempDir::new().expect("create temp dir");
        let manager = NotepadManager::new(tmp.path());
        
        let injected = manager.inject().await.expect("inject should succeed");
        assert_eq!(injected, "");
    }
}
