//! Shared types for the Web module.

/// Result of a web search.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Result of fetching a URL.
#[derive(Debug, Clone)]
pub struct FetchResult {
    pub url: String,
    pub content: String,
    pub content_type: String,
}
