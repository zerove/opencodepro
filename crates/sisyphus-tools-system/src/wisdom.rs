//! Cross-session knowledge retention (Wisdom/Notepads).
//!
//! This module re-exports the high-level `NotepadManager` from
//! `sisyphus_tools_core::wisdom`, and provides a thin compatibility layer
//! for the original `wisdom_record/list/get/delete/inject` free functions
//! (category is a string, content is a single body) used by the CLI and
//! developer agent.
//!
//! ## Storage layout
//!
//! `.omo/notepads/{category}/{name}.md` — one Markdown file per entry, with
//! YAML front-matter (timestamp + tags). `record(category, content)` is
//! sugar for `record(category, "auto", content, vec![])`.

pub use sisyphus_tools_core::wisdom::{
    NotepadCategory, NotepadCategory as Category, NotepadEntry, NotepadManager,
};

use sisyphus_tools_core::{ToolError, ToolResult};
use std::time::{SystemTime, UNIX_EPOCH};

fn parse_category_or_experience(category: &str) -> NotepadCategory {
    NotepadCategory::parse(category).unwrap_or(NotepadCategory::Experience)
}

/// Append a wisdom entry under the given category.
pub async fn wisdom_record(category: &str, content: &str) -> ToolResult<()> {
    let cat = parse_category_or_experience(category);
    let name = format!(
        "auto_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    let tags = if NotepadCategory::parse(category).is_none() {
        vec![category.to_string()]
    } else {
        Vec::new()
    };
    NotepadManager::default_dir()
        .record(cat, &name, content, tags)
        .await
        .map(|_| ())
}

pub async fn wisdom_list() -> ToolResult<Vec<String>> {
    let entries = NotepadManager::default_dir().list_all().await?;
    let mut cats: Vec<String> = entries
        .into_iter()
        .map(|e| e.category.label().to_string())
        .collect();
    cats.sort();
    cats.dedup();
    Ok(cats)
}

pub async fn wisdom_get(category: &str) -> ToolResult<String> {
    let cat = NotepadCategory::parse(category)
        .ok_or_else(|| ToolError::NotFound(format!("unknown wisdom category: {category}")))?;
    let entries = NotepadManager::default_dir().list(cat).await?;
    if entries.is_empty() {
        return Err(ToolError::NotFound(format!(
            "no entries in category: {category}"
        )));
    }
    Ok(entries
        .into_iter()
        .map(|e| format!("### {}\n{}", e.name, e.content))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

pub async fn wisdom_delete(category: &str) -> ToolResult<()> {
    let cat = NotepadCategory::parse(category)
        .ok_or_else(|| ToolError::NotFound(format!("unknown wisdom category: {category}")))?;
    let entries = NotepadManager::default_dir().list(cat).await?;
    for entry in entries {
        let _ = NotepadManager::default_dir()
            .delete(entry.category, &entry.name)
            .await;
    }
    Ok(())
}

pub async fn wisdom_inject() -> ToolResult<String> {
    NotepadManager::default_dir().inject(None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("sisyphus_wisdom_sys_test_{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[tokio::test]
    async fn test_record_and_list_via_manager() {
        let dir = unique_dir();
        let mgr = NotepadManager::new(&dir);
        mgr.record(
            NotepadCategory::Conventions,
            "rule1",
            "use async/await",
            vec![],
        )
        .await
        .expect("record");
        let entries = mgr.list(NotepadCategory::Conventions).await.expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "rule1");
        assert!(entries[0].content.contains("async/await"));
    }

    #[test]
    fn test_parse_category_known() {
        assert_eq!(
            parse_category_or_experience("conventions"),
            NotepadCategory::Conventions
        );
        assert_eq!(
            parse_category_or_experience("patterns"),
            NotepadCategory::Patterns
        );
    }

    #[test]
    fn test_parse_category_unknown_falls_back() {
        assert_eq!(
            parse_category_or_experience("mystery"),
            NotepadCategory::Experience
        );
    }

    #[tokio::test]
    async fn test_inject_via_manager() {
        let dir = unique_dir();
        let mgr = NotepadManager::new(&dir);
        mgr.record(NotepadCategory::Conventions, "c1", "rule", vec![])
            .await
            .expect("record");
        mgr.record(NotepadCategory::Successes, "s1", "win", vec![])
            .await
            .expect("record");
        let injected = mgr.inject(None).await.expect("inject");
        assert!(injected.contains("<wisdom>"));
        assert!(injected.contains("c1"));
        assert!(injected.contains("s1"));
    }
}
