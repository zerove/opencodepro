/// Structured status of a Git working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub staged: Vec<FileStatus>,
    pub unstaged: Vec<FileStatus>,
    pub untracked: Vec<String>,
}

/// Status of a single file in the working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    pub path: String,
    /// One‑character status code: M modified, A added, D deleted, R renamed, ? untracked, etc.
    pub x: char, // index (staging area)
    pub y: char, // working tree
}

/// Structured commit log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitLog {
    pub entries: Vec<CommitEntry>,
}

/// A single commit entry in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

/// Information about a single branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub current: bool,
    pub name: String,
}
