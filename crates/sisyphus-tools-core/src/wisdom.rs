//! Wisdom accumulation: persistent notepads for cross-session knowledge.
//!
//! ## Layout
//!
//! Notepads live in `.omo/notepads/{category}/{name}.md`. Each entry is a
//! Markdown file with YAML-like front-matter (timestamp + tags) plus free-form
//! body. This format is both human-readable and machine-parseable.
//!
//! ## Categories
//!
//! - `conventions` — project style/architecture conventions
//! - `successes`  — what worked well (patterns, fixes, optimizations)
//! - `failures`   — what did not work (avoid in future)
//! - `patterns`   — reusable code/logic patterns
//! - `experience` — domain knowledge, project facts
//!
//! ## Inject
//!
//! `NotepadManager::inject()` returns a single formatted block wrapped in
//! `<wisdom>...</wisdom>` tags, suitable for appending to a system prompt
//! or LLM context window.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;

use crate::ToolResult;

/// All valid notepad categories.
///
/// Stored as filesystem subdirectory names under `.omo/notepads/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NotepadCategory {
    Conventions,
    Successes,
    Failures,
    Patterns,
    Experience,
}

impl NotepadCategory {
    /// Subdirectory name (lowercase, no spaces).
    pub fn as_dir(&self) -> &'static str {
        match self {
            Self::Conventions => "conventions",
            Self::Successes => "successes",
            Self::Failures => "failures",
            Self::Patterns => "patterns",
            Self::Experience => "experience",
        }
    }

    /// Human-readable label for CLI display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Conventions => "conventions",
            Self::Successes => "successes",
            Self::Failures => "failures",
            Self::Patterns => "patterns",
            Self::Experience => "experience",
        }
    }

    /// Parse from a string, case-insensitive.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "conventions" | "convention" | "conv" => Some(Self::Conventions),
            "successes" | "success" | "win" | "wins" => Some(Self::Successes),
            "failures" | "failure" | "fail" | "fails" => Some(Self::Failures),
            "patterns" | "pattern" | "pat" => Some(Self::Patterns),
            "experience" | "exp" | "knowledge" => Some(Self::Experience),
            _ => None,
        }
    }

    pub const ALL: &'static [NotepadCategory] = &[
        Self::Conventions,
        Self::Successes,
        Self::Failures,
        Self::Patterns,
        Self::Experience,
    ];
}

/// A single notepad entry, persisted as one Markdown file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotepadEntry {
    pub name: String,
    pub category: NotepadCategory,
    /// Unix epoch seconds.
    pub timestamp: i64,
    pub tags: Vec<String>,
    /// Free-form Markdown body.
    pub content: String,
}

impl NotepadEntry {
    pub fn new(
        category: NotepadCategory,
        name: impl Into<String>,
        content: impl Into<String>,
        tags: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            category,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            tags,
            content: content.into(),
        }
    }

    /// Render as Markdown with front-matter.
    pub fn to_markdown(&self) -> String {
        let tags_str = if self.tags.is_empty() {
            String::new()
        } else {
            format!("tags: {}\n", self.tags.join(", "))
        };
        format!(
            "---\nname: {}\ncategory: {}\ntimestamp: {}\n{}---\n\n{}\n",
            self.name,
            self.category.as_dir(),
            self.timestamp,
            tags_str,
            self.content.trim_end(),
        )
    }

    /// Parse a Markdown file into a NotepadEntry.
    /// Front-matter is optional; if absent, name/category must be supplied by caller.
    pub fn from_markdown(name: String, category: NotepadCategory, raw: &str) -> Self {
        let raw = raw.trim_start_matches('\u{feff}'); // strip BOM
        let mut timestamp: i64 = 0;
        let mut tags: Vec<String> = Vec::new();
        let mut content = raw.to_string();

        if let Some(stripped) = raw.strip_prefix("---") {
            if let Some(end_idx) = stripped.find("\n---") {
                let front_matter = &stripped[..end_idx];
                let after = &stripped[end_idx + 4..];
                for line in front_matter.lines() {
                    let line = line.trim();
                    if let Some(rest) = line.strip_prefix("timestamp:") {
                        if let Ok(n) = rest.trim().parse() {
                            timestamp = n;
                        }
                    } else if let Some(rest) = line.strip_prefix("tags:") {
                        tags = rest
                            .trim()
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                content = after.trim_start_matches('\n').to_string();
            }
        }

        Self {
            name,
            category,
            timestamp,
            tags,
            content,
        }
    }
}

/// Persistent store for notepads, organized as one Markdown file per entry.
#[derive(Debug, Clone)]
pub struct NotepadManager {
    base_path: PathBuf,
}

impl NotepadManager {
    /// Create a manager rooted at the given directory (e.g. `.omo/notepads`).
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            base_path: path.as_ref().to_path_buf(),
        }
    }

    /// Default location: `.omo/notepads` relative to CWD.
    pub fn default_dir() -> Self {
        Self::new(".omo/notepads")
    }

    fn entry_path(&self, category: NotepadCategory, name: &str) -> PathBuf {
        // Sanitize name: forbid path separators and ".." to prevent escape.
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
            return self.base_path.join("__invalid__");
        }
        self.base_path.join(category.as_dir()).join(format!("{name}.md"))
    }

    /// Record a new notepad entry. Overwrites if the same (category, name) exists.
    pub async fn record(
        &self,
        category: NotepadCategory,
        name: &str,
        content: &str,
        tags: Vec<String>,
    ) -> ToolResult<NotepadEntry> {
        let entry = NotepadEntry::new(category, name, content, tags);
        let path = self.entry_path(category, name);

        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }
        }
        fs::write(&path, entry.to_markdown()).await?;
        Ok(entry)
    }

    /// Fetch a single entry by (category, name).
    pub async fn get(
        &self,
        category: NotepadCategory,
        name: &str,
    ) -> ToolResult<Option<NotepadEntry>> {
        let path = self.entry_path(category, name);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path).await?;
        Ok(Some(NotepadEntry::from_markdown(
            name.to_string(),
            category,
            &raw,
        )))
    }

    /// List all entries across all categories, sorted by timestamp descending.
    pub async fn list_all(&self) -> ToolResult<Vec<NotepadEntry>> {
        let mut out = Vec::new();
        if !self.base_path.exists() {
            return Ok(out);
        }
        for category in NotepadCategory::ALL {
            out.extend(self.list(*category).await?);
        }
        out.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
        Ok(out)
    }

    /// List entries in a single category, sorted by timestamp descending.
    pub async fn list(&self, category: NotepadCategory) -> ToolResult<Vec<NotepadEntry>> {
        let dir = self.base_path.join(category.as_dir());
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let mut entries = fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let raw = fs::read_to_string(&path).await?;
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            out.push(NotepadEntry::from_markdown(name, category, &raw));
        }
        out.sort_by(|a, b| {
            std::cmp::Reverse(a.timestamp)
                .cmp(&std::cmp::Reverse(b.timestamp))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(out)
    }

    /// Delete an entry. Returns Ok(true) if removed, Ok(false) if absent.
    pub async fn delete(&self, category: NotepadCategory, name: &str) -> ToolResult<bool> {
        let path = self.entry_path(category, name);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path).await?;
        Ok(true)
    }

    /// Build an injected `<wisdom>` block for LLM context.
    /// If `limit_per_category` is `Some(n)`, at most n entries per category are included.
    /// If `None`, all entries are included.
    pub async fn inject(&self, limit_per_category: Option<usize>) -> ToolResult<String> {
        let mut sections: Vec<String> = Vec::new();
        for category in NotepadCategory::ALL {
            let entries = match self.list(*category).await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let entries: Vec<NotepadEntry> = match limit_per_category {
                Some(n) => entries.into_iter().take(n).collect(),
                None => entries,
            };
            if entries.is_empty() {
                continue;
            }
            let mut section = format!("## {} ({} entries)\n", category.label(), entries.len());
            for entry in entries {
                let tags_str = if entry.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", entry.tags.join(", "))
                };
                section.push_str(&format!(
                    "### {} (ts={}){}\n{}\n\n",
                    entry.name, entry.timestamp, tags_str, entry.content
                ));
            }
            sections.push(section);
        }
        if sections.is_empty() {
            Ok(String::new())
        } else {
            Ok(format!(
                "<wisdom>\nThe following notepads are persistent knowledge accumulated from prior sessions.\nUse them as context when relevant; do not blindly follow them if the current situation differs.\n\n{}</wisdom>\n",
                sections.join("\n")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("sisyphus_wisdom_test_{n}"));
        // Best-effort cleanup of leftover dir from previous run.
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn ts_for(entry: &NotepadEntry) -> i64 {
        entry.timestamp
    }

    #[test]
    fn test_category_parse() {
        assert_eq!(
            NotepadCategory::parse("conventions"),
            Some(NotepadCategory::Conventions)
        );
        assert_eq!(
            NotepadCategory::parse("CONVENTIONS"),
            Some(NotepadCategory::Conventions)
        );
        assert_eq!(
            NotepadCategory::parse("conv"),
            Some(NotepadCategory::Conventions)
        );
        assert_eq!(
            NotepadCategory::parse("successes"),
            Some(NotepadCategory::Successes)
        );
        assert_eq!(
            NotepadCategory::parse("win"),
            Some(NotepadCategory::Successes)
        );
        assert_eq!(
            NotepadCategory::parse("failures"),
            Some(NotepadCategory::Failures)
        );
        assert_eq!(
            NotepadCategory::parse("patterns"),
            Some(NotepadCategory::Patterns)
        );
        assert_eq!(
            NotepadCategory::parse("experience"),
            Some(NotepadCategory::Experience)
        );
        assert_eq!(NotepadCategory::parse("unknown"), None);
    }

    #[test]
    fn test_category_as_dir() {
        assert_eq!(NotepadCategory::Conventions.as_dir(), "conventions");
        assert_eq!(NotepadCategory::Successes.as_dir(), "successes");
        assert_eq!(NotepadCategory::Failures.as_dir(), "failures");
        assert_eq!(NotepadCategory::Patterns.as_dir(), "patterns");
        assert_eq!(NotepadCategory::Experience.as_dir(), "experience");
    }

    #[test]
    fn test_notepad_entry_markdown_roundtrip() {
        let entry = NotepadEntry {
            name: "test".into(),
            category: NotepadCategory::Patterns,
            timestamp: 1234567890,
            tags: vec!["rust".into(), "async".into()],
            content: "Use tokio::spawn for fire-and-forget tasks.".into(),
        };
        let md = entry.to_markdown();
        let parsed = NotepadEntry::from_markdown("test".into(), NotepadCategory::Patterns, &md);
        assert_eq!(parsed.name, "test");
        assert_eq!(parsed.category, NotepadCategory::Patterns);
        assert_eq!(parsed.timestamp, 1234567890);
        assert_eq!(parsed.tags, vec!["rust", "async"]);
        assert!(parsed.content.contains("tokio::spawn"));
    }

    #[test]
    fn test_notepad_entry_no_front_matter() {
        let entry = NotepadEntry::from_markdown(
            "x".into(),
            NotepadCategory::Conventions,
            "Just plain markdown body.\n",
        );
        assert_eq!(entry.timestamp, 0);
        assert!(entry.tags.is_empty());
        assert!(entry.content.contains("plain markdown"));
    }

    #[tokio::test]
    async fn test_record_creates_file() {
        let dir = unique_temp_dir();
        let mgr = NotepadManager::new(&dir);
        let entry = mgr
            .record(
                NotepadCategory::Conventions,
                "test_convention",
                "always run cargo fmt",
                vec!["rust".into()],
            )
            .await
            .expect("record");
        assert!(ts_for(&entry) > 0);
        let path = dir.join("conventions").join("test_convention.md");
        assert!(path.exists());
    }

    #[tokio::test]
    async fn test_list_includes_recorded() {
        let dir = unique_temp_dir();
        let mgr = NotepadManager::new(&dir);
        mgr.record(NotepadCategory::Conventions, "c1", "body1", vec![]).await.unwrap();
        mgr.record(NotepadCategory::Patterns, "p1", "body2", vec![]).await.unwrap();
        let all = mgr.list_all().await.unwrap();
        assert_eq!(all.len(), 2);
        let convs = mgr.list(NotepadCategory::Conventions).await.unwrap();
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0].name, "c1");
    }

    #[tokio::test]
    async fn test_inject_contains_tags() {
        let dir = unique_temp_dir();
        let mgr = NotepadManager::new(&dir);
        mgr.record(
            NotepadCategory::Conventions,
            "rust_fmt",
            "Run cargo fmt before commit",
            vec!["rust".into(), "ci".into()],
        )
        .await
        .unwrap();
        let injected = mgr.inject(Some(5)).await.unwrap();
        assert!(injected.contains("<wisdom>"));
        assert!(injected.contains("</wisdom>"));
        assert!(injected.contains("rust_fmt"));
        assert!(injected.contains("[rust, ci]"));
        assert!(injected.contains("conventions"));
    }

    #[tokio::test]
    async fn test_inject_limit_per_category() {
        let dir = unique_temp_dir();
        let mgr = NotepadManager::new(&dir);
        // 直接写入带确定性时间戳的条目，而不是连续调用 `record`。
        //
        // `record` 用 `SystemTime::now().as_secs()` 打时间戳，精度只有秒。
        // 连续 5 次写入极快，只要恰好跨过秒边界，最先写的 p0 就会落到较早的
        // 那一秒，被 `inject(Some(2))` 的 `take(2)` 排除，断言 `contains("p0")`
        // 随之失败。此缺陷实测约 1/80 复现：单次 `cargo test` 几乎必然全绿，
        // 只有高频复跑才暴露。显式 sleep 1.1s（如同文件的 `test_overwrite`）
        // 能绕过，但会让每次运行都多花 5 秒；写死时间戳则完全确定且零开销。
        //
        // 时间戳从大到小，故 p0 最新、p4 最旧。
        let base_ts = 1_000_000i64;
        for i in 0..5 {
            let entry = NotepadEntry {
                name: format!("p{i}"),
                category: NotepadCategory::Patterns,
                timestamp: base_ts - i as i64,
                tags: vec![],
                content: "body".into(),
            };
            let path = mgr.entry_path(NotepadCategory::Patterns, &entry.name);
            tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
            tokio::fs::write(&path, entry.to_markdown()).await.unwrap();
        }
        let injected = mgr.inject(Some(2)).await.unwrap();
        assert!(injected.contains("p0"), "应保留最新的 p0，实际: {injected}");
        assert!(injected.contains("p1"), "应保留次新的 p1，实际: {injected}");
        assert!(!injected.contains("p4"), "limit=2 应排除较旧的 p4");
    }

    #[tokio::test]
    async fn test_inject_empty_when_no_entries() {
        let dir = unique_temp_dir();
        let mgr = NotepadManager::new(&dir);
        let injected = mgr.inject(None).await.unwrap();
        assert!(injected.is_empty());
    }

    #[tokio::test]
    async fn test_get_returns_recorded() {
        let dir = unique_temp_dir();
        let mgr = NotepadManager::new(&dir);
        mgr.record(NotepadCategory::Successes, "s1", "fix", vec!["perf".into()]).await.unwrap();
        let entry = mgr.get(NotepadCategory::Successes, "s1").await.unwrap();
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.name, "s1");
        assert_eq!(
            entry.content,
            "fix
"
        );
        assert_eq!(entry.tags, vec!["perf"]);
    }

    #[tokio::test]
    async fn test_get_absent() {
        let dir = unique_temp_dir();
        let mgr = NotepadManager::new(&dir);
        let entry = mgr.get(NotepadCategory::Failures, "missing").await.unwrap();
        assert!(entry.is_none());
    }

    #[tokio::test]
    async fn test_delete() {
        let dir = unique_temp_dir();
        let mgr = NotepadManager::new(&dir);
        mgr.record(NotepadCategory::Patterns, "to_delete", "x", vec![]).await.unwrap();
        assert!(mgr.delete(NotepadCategory::Patterns, "to_delete").await.unwrap());
        // Idempotent: second call returns false.
        assert!(!mgr.delete(NotepadCategory::Patterns, "to_delete").await.unwrap());
        assert!(mgr.get(NotepadCategory::Patterns, "to_delete").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_overwrite() {
        let dir = unique_temp_dir();
        let mgr = NotepadManager::new(&dir);
        mgr.record(NotepadCategory::Experience, "fact", "old", vec![]).await.unwrap();
        // Sleep to ensure new timestamp > old.
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        mgr.record(NotepadCategory::Experience, "fact", "new", vec![]).await.unwrap();
        let entry = mgr.get(NotepadCategory::Experience, "fact").await.unwrap().unwrap();
        assert_eq!(
            entry.content,
            "new
"
        );
    }

    #[test]
    fn test_path_traversal_blocked() {
        let dir = unique_temp_dir();
        let mgr = NotepadManager::new(&dir);
        let path = mgr.entry_path(NotepadCategory::Conventions, "../escape");
        // Path is sanitized — should not escape base_path.
        assert!(path.starts_with(&mgr.base_path));
        assert!(path.ends_with("__invalid__"));
    }
}
