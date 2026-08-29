//! Integration tests for sisyphus-tools-scm `git::types` module's
//! pure data types: `GitStatus`, `FileStatus`, `GitCommitLog`, `CommitEntry`.

use sisyphus_tools_scm::git::{CommitEntry, FileStatus, GitCommitLog, GitStatus};

// ---------------------------------------------------------------------------
// FileStatus
// ---------------------------------------------------------------------------

#[test]
fn file_status_field_access() {
    let f = FileStatus {
        path: "src/main.rs".into(),
        x: 'M',
        y: ' ',
    };
    assert_eq!(f.path, "src/main.rs");
    assert_eq!(f.x, 'M');
    assert_eq!(f.y, ' ');
}

#[test]
fn file_status_untracked() {
    let f = FileStatus {
        path: "new.txt".into(),
        x: '?',
        y: '?',
    };
    assert_eq!(f.x, '?');
    assert_eq!(f.y, '?');
}

#[test]
fn file_status_added() {
    let f = FileStatus {
        path: "added".into(),
        x: 'A',
        y: ' ',
    };
    assert_eq!(f.x, 'A');
}

// ---------------------------------------------------------------------------
// CommitEntry
// ---------------------------------------------------------------------------

#[test]
fn commit_entry_field_access() {
    let c = CommitEntry {
        hash: "abc123".into(),
        author: "alice".into(),
        date: "2026-01-01".into(),
        message: "Initial commit".into(),
    };
    assert_eq!(c.hash, "abc123");
    assert_eq!(c.author, "alice");
    assert_eq!(c.message, "Initial commit");
}

#[test]
fn commit_entry_equality() {
    let a = CommitEntry {
        hash: "h".into(),
        author: "a".into(),
        date: "d".into(),
        message: "m".into(),
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn commit_entry_inequality() {
    let a = CommitEntry {
        hash: "h1".into(),
        author: "a".into(),
        date: "d".into(),
        message: "m".into(),
    };
    let b = CommitEntry {
        hash: "h2".into(),
        author: "a".into(),
        date: "d".into(),
        message: "m".into(),
    };
    assert_ne!(a, b);
}

// ---------------------------------------------------------------------------
// GitCommitLog
// ---------------------------------------------------------------------------

#[test]
fn git_commit_log_empty() {
    let l = GitCommitLog { entries: vec![] };
    assert!(l.entries.is_empty());
}

#[test]
fn git_commit_log_with_entries() {
    let l = GitCommitLog {
        entries: vec![
            CommitEntry {
                hash: "h1".into(),
                author: "a".into(),
                date: "d".into(),
                message: "m1".into(),
            },
            CommitEntry {
                hash: "h2".into(),
                author: "b".into(),
                date: "d".into(),
                message: "m2".into(),
            },
        ],
    };
    assert_eq!(l.entries.len(), 2);
    assert_eq!(l.entries[0].hash, "h1");
    assert_eq!(l.entries[1].author, "b");
}

// ---------------------------------------------------------------------------
// GitStatus
// ---------------------------------------------------------------------------

#[test]
fn git_status_field_access() {
    let s = GitStatus {
        branch: "main".into(),
        ahead: 2,
        behind: 1,
        staged: vec![FileStatus {
            path: "a".into(),
            x: 'M',
            y: ' ',
        }],
        unstaged: vec![],
        untracked: vec!["b".into()],
    };
    assert_eq!(s.branch, "main");
    assert_eq!(s.ahead, 2);
    assert_eq!(s.behind, 1);
    assert_eq!(s.staged.len(), 1);
    assert_eq!(s.untracked.len(), 1);
    assert!(s.unstaged.is_empty());
}

#[test]
fn git_status_clean() {
    let s = GitStatus {
        branch: "develop".into(),
        ahead: 0,
        behind: 0,
        staged: vec![],
        unstaged: vec![],
        untracked: vec![],
    };
    assert_eq!(s.branch, "develop");
    assert!(s.staged.is_empty());
    assert!(s.unstaged.is_empty());
    assert!(s.untracked.is_empty());
}

#[test]
fn git_status_clone() {
    let s = GitStatus {
        branch: "x".into(),
        ahead: 0,
        behind: 0,
        staged: vec![],
        unstaged: vec![],
        untracked: vec![],
    };
    let s2 = s.clone();
    assert_eq!(s.branch, s2.branch);
    assert_eq!(s.ahead, s2.ahead);
}
