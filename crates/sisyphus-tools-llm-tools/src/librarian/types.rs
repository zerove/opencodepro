//! Shared types for the Librarian research module.
//!
//! Defines ResearchFinding, ResearchReport, ResearchScope, DocPage, CodeExample.

use serde::{Deserialize, Serialize};

/// A researched fact with source attribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchFinding {
    pub claim: String,
    pub source_url: String,
    pub source_title: String,
    pub relevance: f64, // 0.0 – 1.0
    pub retrieved_at: String,
}

/// Result of a librarian research query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchReport {
    pub query: String,
    pub findings: Vec<ResearchFinding>,
    pub synthesis: String,
    pub sources_consulted: usize,
    pub confidence: f64, // 0.0 – 1.0
}

/// Search scope for the librarian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResearchScope {
    Web,
    Documentation,
    CodeExamples,
    All,
}

impl std::str::FromStr for ResearchScope {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "web" | "www" => Ok(Self::Web),
            "docs" | "documentation" => Ok(Self::Documentation),
            "code" | "examples" => Ok(Self::CodeExamples),
            "all" => Ok(Self::All),
            _ => Err(format!(
                "unknown research scope: {s} (expected web|docs|code|all)"
            )),
        }
    }
}

/// A single documentation page fetched and parsed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocPage {
    pub url: String,
    pub title: String,
    pub content_preview: String, // first ~500 chars
    pub content_length: usize,
}

/// Example code snippet found during research.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeExample {
    pub source: String,
    pub language: String,
    pub code: String,
    pub description: String,
}

/// ISO-8601 timestamp helper.
pub fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.as_millis() % 1000;
    let secs_in_day = secs % 86400;
    let hours = secs_in_day / 3600;
    let minutes = (secs_in_day % 3600) / 60;
    let seconds = secs_in_day % 60;
    let days_since_epoch = secs / 86400;
    let year = 1970 + (days_since_epoch as f64 / 365.25) as u64;
    let month = 1 + ((days_since_epoch % 365) as f64 / 30.44) as u64;
    let day = 1 + ((days_since_epoch % 365) as f64 % 30.44) as u64;
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z")
}
