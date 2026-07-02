use std::path::PathBuf;

/// A match found during exploration with surrounding context.
#[derive(Debug, Clone)]
pub struct ExploreMatch {
    pub file: PathBuf,
    pub line: usize,
    pub content: String,
    /// Lines before the match (context)
    pub before: Vec<String>,
    /// Lines after the match (context)
    pub after: Vec<String>,
    /// Which pattern matched (index into the pattern list)
    pub pattern_index: usize,
}

/// Structured result from a codebase exploration.
#[derive(Debug, Clone)]
pub struct ExploreResult {
    pub matches: Vec<ExploreMatch>,
    pub files_scanned: usize,
    pub total_matches: usize,
}

/// Context from a single file.
#[derive(Debug, Clone)]
pub struct FileContext {
    pub path: PathBuf,
    pub content: String,
    pub line_count: usize,
}
