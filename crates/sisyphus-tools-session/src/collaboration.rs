//! Session collaboration: version history, diff/merge, and shared sessions.
//!
//! Provides:
//! - `SessionDiff` — structured diff between two session versions
//! - `SessionVersion` — a versioned snapshot of a session
//! - `SharedSession` — a session that can be shared between users/instances
//! - Merge operations for combining diverged session branches

use serde::{Deserialize, Serialize};
use sisyphus_tools_core::{ToolError, ToolResult};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::session::types::{now_iso, Message, Session};

// ─── Version History ───────────────────────────────────────────────────

/// A versioned snapshot of a session at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionVersion {
    /// Version identifier (incrementing integer or UUID).
    pub version: String,
    /// Session ID this version belongs to.
    pub session_id: String,
    /// Snapshot of messages at this version.
    pub messages: Vec<Message>,
    /// Snapshot of session metadata at this version.
    pub metadata: HashMap<String, String>,
    /// Timestamp when this version was created.
    pub created_at: String,
    /// Optional commit message describing the change.
    pub commit_message: Option<String>,
}

/// Version history for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionHistory {
    /// Session ID.
    pub session_id: String,
    /// All versions in chronological order.
    pub versions: Vec<SessionVersion>,
}

impl VersionHistory {
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            versions: Vec::new(),
        }
    }

    /// Create a new version from the current session state.
    pub fn snapshot(session: &Session, commit_message: Option<String>) -> SessionVersion {
        let seq = now_iso(); // Use timestamp as version identifier
        SessionVersion {
            version: seq.clone(),
            session_id: session.id.clone(),
            messages: session.messages.clone(),
            metadata: session.metadata.clone(),
            created_at: seq,
            commit_message,
        }
    }

    /// Load version history from disk.
    pub fn load(path: &Path, session_id: &str) -> ToolResult<Self> {
        let history_path = path
            .parent()
            .ok_or_else(|| ToolError::TodoError("invalid path".into()))?
            .join("versions")
            .join(format!("{session_id}.json"));

        if !history_path.exists() {
            return Ok(Self::new(session_id));
        }

        let content = fs::read_to_string(&history_path).map_err(ToolError::Io)?;
        serde_json::from_str(&content)
            .map_err(|e| ToolError::TodoError(format!("deserialize version history: {e}")))
    }

    /// Save version history to disk.
    pub fn save(&self, path: &Path) -> ToolResult<()> {
        let versions_dir = path
            .parent()
            .ok_or_else(|| ToolError::TodoError("invalid path".into()))?
            .join("versions");
        fs::create_dir_all(&versions_dir).map_err(ToolError::Io)?;

        let history_path = versions_dir.join(format!("{}.json", self.session_id));
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ToolError::TodoError(format!("serialize version history: {e}")))?;
        fs::write(history_path, content).map_err(ToolError::Io)
    }

    /// Push a new version (removes any versions after current index).
    pub fn push(&mut self, version: SessionVersion) {
        self.versions.push(version);
    }

    /// Get a specific version by version string.
    pub fn get_version(&self, version: &str) -> Option<&SessionVersion> {
        self.versions.iter().find(|v| v.version == version)
    }

    /// Get the latest version.
    pub fn latest(&self) -> Option<&SessionVersion> {
        self.versions.last()
    }

    /// Count versions.
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    /// Check if version history is empty.
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }
}

// ─── Diff / Merge ─────────────────────────────────────────────────────

/// A single diff operation between two message lists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DiffOp {
    /// Insert at index.
    Insert { index: usize, message: Message },
    /// Delete at index.
    Delete { index: usize, message: Message },
    /// Modify message at index.
    Modify {
        index: usize,
        old_message: Message,
        new_message: Message,
    },
}

/// A structured diff between two session versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDiff {
    /// Session ID.
    pub session_id: String,
    /// List of operations to transform the first version into the second.
    pub operations: Vec<DiffOp>,
    /// Metadata changes (key-value pairs added, modified, or removed).
    pub metadata_changes: HashMap<String, Option<String>>,
}

impl SessionDiff {
    /// Compute a diff between two message slices.
    /// A simple heuristic: match messages by content beyond a similarity threshold.
    pub fn compute(
        session_id: &str,
        old_messages: &[Message],
        new_messages: &[Message],
        old_metadata: &HashMap<String, String>,
        new_metadata: &HashMap<String, String>,
    ) -> Self {
        let mut operations = Vec::new();

        // Simple diff: compare message by message up to the common prefix length
        let common_len = old_messages.len().min(new_messages.len());

        // Check for modifications in common prefix
        for i in 0..common_len {
            if old_messages[i].content != new_messages[i].content
                || old_messages[i].role != new_messages[i].role
            {
                operations.push(DiffOp::Modify {
                    index: i,
                    old_message: old_messages[i].clone(),
                    new_message: new_messages[i].clone(),
                });
            }
        }

        // Extra messages in old = deletions
        for (i, msg) in old_messages.iter().enumerate().skip(common_len) {
            operations.push(DiffOp::Delete {
                index: i,
                message: msg.clone(),
            });
        }

        // Extra messages in new = insertions
        for (i, msg) in new_messages.iter().enumerate().skip(common_len) {
            operations.push(DiffOp::Insert {
                index: i,
                message: msg.clone(),
            });
        }

        // Compute metadata changes
        let mut metadata_changes: HashMap<String, Option<String>> = HashMap::new();

        // Keys removed or changed
        for (key, old_val) in old_metadata {
            match new_metadata.get(key) {
                Some(new_val) if new_val != old_val => {
                    metadata_changes.insert(key.clone(), Some(new_val.clone()));
                }
                Some(_) => { /* unchanged */ }
                None => {
                    metadata_changes.insert(key.clone(), None);
                }
            }
        }

        // Keys added
        for (key, new_val) in new_metadata {
            if !old_metadata.contains_key(key) {
                metadata_changes.insert(key.clone(), Some(new_val.clone()));
            }
        }

        Self {
            session_id: session_id.to_string(),
            operations,
            metadata_changes,
        }
    }

    /// Apply this diff to a session, producing a new session.
    pub fn apply(&self, session: &Session) -> ToolResult<Session> {
        let mut messages = session.messages.clone();

        // Process operations in order: deletions first (reverse to maintain indices),
        // then modifications, then insertions.
        let mut deletes: Vec<&DiffOp> = Vec::new();
        let mut modifies: Vec<&DiffOp> = Vec::new();
        let mut inserts: Vec<&DiffOp> = Vec::new();

        for op in &self.operations {
            match op {
                DiffOp::Delete { .. } => deletes.push(op),
                DiffOp::Modify { .. } => modifies.push(op),
                DiffOp::Insert { .. } => inserts.push(op),
            }
        }

        // Apply deletions in reverse order
        for op in deletes.iter().rev() {
            if let DiffOp::Delete { index, .. } = op {
                if *index < messages.len() {
                    messages.remove(*index);
                } else {
                    return Err(ToolError::TodoError(format!(
                        "diff apply: delete index {index} out of bounds (len {})",
                        messages.len()
                    )));
                }
            }
        }

        // Apply modifications
        for op in &modifies {
            if let DiffOp::Modify {
                index, new_message, ..
            } = op
            {
                if *index < messages.len() {
                    messages[*index] = new_message.clone();
                } else {
                    return Err(ToolError::TodoError(format!(
                        "diff apply: modify index {index} out of bounds (len {})",
                        messages.len()
                    )));
                }
            }
        }

        // Apply insertions (forward order)
        for op in &inserts {
            if let DiffOp::Insert { index, message } = op {
                let insert_at = (*index).min(messages.len());
                messages.insert(insert_at, message.clone());
            }
        }

        // Apply metadata changes
        let mut metadata = session.metadata.clone();
        for (key, value) in &self.metadata_changes {
            match value {
                Some(v) => {
                    metadata.insert(key.clone(), v.clone());
                }
                None => {
                    metadata.remove(key);
                }
            }
        }

        let message_count = messages.len() as u64;

        Ok(Session {
            messages,
            metadata,
            message_count,
            ..session.clone()
        })
    }

    /// Check if the diff is empty (no changes).
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty() && self.metadata_changes.is_empty()
    }
}

// ─── Shared Sessions ──────────────────────────────────────────────────

/// Access level for a shared session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccessLevel {
    /// Read-only access.
    Read,
    /// Read + comment (add messages) access.
    Comment,
    /// Full write access.
    Write,
    /// Owner — full access including sharing.
    Owner,
}

/// A shared session invitation or link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedSession {
    /// The original session ID.
    pub session_id: String,
    /// Unique share token or link ID.
    pub share_id: String,
    /// Access level granted.
    pub access_level: AccessLevel,
    /// Optional list of allowed user IDs (empty = anyone with link).
    pub allowed_users: Vec<String>,
    /// Timestamp when the share was created.
    pub created_at: String,
    /// Optional expiry timestamp.
    pub expires_at: Option<String>,
}

/// Registry of shared sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedSessionRegistry {
    /// Map of share_id -> SharedSession.
    pub shares: HashMap<String, SharedSession>,
    /// Map of session_id -> Vec<share_id> for reverse lookup.
    pub session_shares: HashMap<String, Vec<String>>,
}

impl SharedSessionRegistry {
    pub fn new() -> Self {
        Self {
            shares: HashMap::new(),
            session_shares: HashMap::new(),
        }
    }
}

impl Default for SharedSessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedSessionRegistry {
    /// Load the registry from disk.
    pub fn load(path: &Path) -> ToolResult<Self> {
        let registry_path = path
            .parent()
            .ok_or_else(|| ToolError::TodoError("invalid path".into()))?
            .join("shared_sessions.json");

        if !registry_path.exists() {
            return Ok(Self::new());
        }

        let content = fs::read_to_string(&registry_path).map_err(ToolError::Io)?;
        serde_json::from_str(&content)
            .map_err(|e| ToolError::TodoError(format!("deserialize shared registry: {e}")))
    }

    /// Save the registry to disk.
    pub fn save(&self, path: &Path) -> ToolResult<()> {
        let registry_path = path
            .parent()
            .ok_or_else(|| ToolError::TodoError("invalid path".into()))?
            .join("shared_sessions.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ToolError::TodoError(format!("serialize shared registry: {e}")))?;
        fs::write(registry_path, content).map_err(ToolError::Io)
    }

    /// Share a session and return a share token.
    pub fn share(
        &mut self,
        session_id: &str,
        access_level: AccessLevel,
        allowed_users: Vec<String>,
        expires_at: Option<String>,
    ) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        session_id.hash(&mut hasher);
        now_iso().hash(&mut hasher);
        let share_id = format!("share_{:x}", hasher.finish());

        let share = SharedSession {
            session_id: session_id.to_string(),
            share_id: share_id.clone(),
            access_level,
            allowed_users,
            created_at: now_iso(),
            expires_at,
        };

        self.shares.insert(share_id.clone(), share);

        self.session_shares
            .entry(session_id.to_string())
            .or_default()
            .push(share_id.clone());

        share_id
    }

    /// Look up a shared session by share token.
    pub fn lookup(&self, share_id: &str) -> Option<&SharedSession> {
        self.shares.get(share_id)
    }

    /// Revoke a share.
    pub fn revoke(&mut self, share_id: &str) -> Option<SharedSession> {
        let share = self.shares.remove(share_id)?;
        if let Some(share_ids) = self.session_shares.get_mut(&share.session_id) {
            share_ids.retain(|id| id != share_id);
            if share_ids.is_empty() {
                self.session_shares.remove(&share.session_id);
            }
        }
        Some(share)
    }

    /// List all shares for a session.
    pub fn list_for_session(&self, session_id: &str) -> Vec<&SharedSession> {
        self.session_shares
            .get(session_id)
            .map(|ids| ids.iter().filter_map(|id| self.shares.get(id)).collect())
            .unwrap_or_default()
    }

    /// Check if a share token is valid for a given user.
    pub fn check_access(&self, share_id: &str, user_id: &str) -> Option<AccessLevel> {
        let share = self.shares.get(share_id)?;

        // Check expiry
        if let Some(expires_at) = &share.expires_at {
            if expires_at.as_str() < now_iso().as_str() {
                return None; // Expired
            }
        }

        // Check allowed users
        if !share.allowed_users.is_empty() && !share.allowed_users.contains(&user_id.to_string()) {
            return None; // Not authorized
        }

        Some(share.access_level.clone())
    }
}

// ─── Session Branching ────────────────────────────────────────────────

/// Represents a branch in session history (like git branches).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBranch {
    /// Branch name.
    pub name: String,
    /// Parent session ID.
    pub session_id: String,
    /// Version pointer to the branch tip.
    pub tip_version: String,
    /// When the branch was created.
    pub created_at: String,
    /// Optional description.
    pub description: Option<String>,
}

/// Branch registry for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBranches {
    /// Session ID.
    pub session_id: String,
    /// Active branch name.
    pub active_branch: String,
    /// All branches.
    pub branches: HashMap<String, SessionBranch>,
}

impl SessionBranches {
    pub fn new(session_id: &str) -> Self {
        let main = SessionBranch {
            name: "main".to_string(),
            session_id: session_id.to_string(),
            tip_version: "0".to_string(),
            created_at: now_iso(),
            description: Some("Main branch".to_string()),
        };
        let mut branches = HashMap::new();
        branches.insert("main".to_string(), main);

        Self {
            session_id: session_id.to_string(),
            active_branch: "main".to_string(),
            branches,
        }
    }

    /// Load branches from disk.
    pub fn load(path: &Path, session_id: &str) -> ToolResult<Self> {
        let branches_path = path
            .parent()
            .ok_or_else(|| ToolError::TodoError("invalid path".into()))?
            .join("branches")
            .join(format!("{session_id}.json"));

        if !branches_path.exists() {
            return Ok(Self::new(session_id));
        }

        let content = fs::read_to_string(&branches_path).map_err(ToolError::Io)?;
        serde_json::from_str(&content)
            .map_err(|e| ToolError::TodoError(format!("deserialize branches: {e}")))
    }

    /// Save branches to disk.
    pub fn save(&self, path: &Path) -> ToolResult<()> {
        let branches_dir = path
            .parent()
            .ok_or_else(|| ToolError::TodoError("invalid path".into()))?
            .join("branches");
        fs::create_dir_all(&branches_dir).map_err(ToolError::Io)?;

        let branches_path = branches_dir.join(format!("{}.json", self.session_id));
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ToolError::TodoError(format!("serialize branches: {e}")))?;
        fs::write(branches_path, content).map_err(ToolError::Io)
    }

    /// Create a new branch from the current branch's tip.
    pub fn create_branch(
        &mut self,
        name: &str,
        description: Option<String>,
        tip_version: &str,
    ) -> ToolResult<()> {
        if self.branches.contains_key(name) {
            return Err(ToolError::TodoError(format!(
                "branch '{name}' already exists"
            )));
        }

        let branch = SessionBranch {
            name: name.to_string(),
            session_id: self.session_id.clone(),
            tip_version: tip_version.to_string(),
            created_at: now_iso(),
            description,
        };

        self.branches.insert(name.to_string(), branch);
        Ok(())
    }

    /// Switch to a different branch.
    pub fn switch(&mut self, name: &str) -> ToolResult<()> {
        if !self.branches.contains_key(name) {
            return Err(ToolError::TodoError(format!("branch '{name}' not found")));
        }
        self.active_branch = name.to_string();
        Ok(())
    }

    /// Get the active branch.
    pub fn active(&self) -> &SessionBranch {
        &self.branches[&self.active_branch]
    }
}

/// Merge two branches of a session.
/// Strategy: simple append — merge combines messages from both branches.
/// In a real implementation, this would do three-way merge with conflict resolution.
pub fn merge_branches(
    base: &[Message],
    branch_a: &[Message],
    branch_b: &[Message],
) -> Vec<Message> {
    let base_len = base.len();

    // If both branches only appended, combine both sets of new messages
    if branch_a.len() >= base_len && branch_b.len() >= base_len {
        let mut merged = base.to_vec();

        // Messages unique to branch A (after base)
        let a_only = &branch_a[base_len..];
        // Messages unique to branch B (after base)
        let b_only = &branch_b[base_len..];

        // Interleave messages from both branches
        let max_extra = a_only.len().max(b_only.len());
        for i in 0..max_extra {
            if i < a_only.len() {
                merged.push(a_only[i].clone());
            }
            if i < b_only.len() {
                merged.push(b_only[i].clone());
            }
        }

        return merged;
    }

    // If one branch deleted/modified messages, prefer branch A (ours) by default
    // and append any extra messages from branch B
    let mut merged = branch_a.to_vec();
    if branch_b.len() > branch_a.len() {
        for msg in &branch_b[branch_a.len()..] {
            merged.push(msg.clone());
        }
    }

    merged
}

// ─── Session Collaboration Events ─────────────────────────────────────

/// An event in the session collaboration log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationEvent {
    /// Event ID.
    pub id: String,
    /// Session ID.
    pub session_id: String,
    /// User or agent who performed the action.
    pub actor: String,
    /// Type of event.
    pub event_type: CollaborationEventType,
    /// Timestamp.
    pub timestamp: String,
}

/// Types of collaboration events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CollaborationEventType {
    /// Session was forked.
    Forked { new_session_id: String },
    /// Session was branched.
    Branched { branch_name: String },
    /// Branch was merged.
    Merged {
        from_branch: String,
        into_branch: String,
    },
    /// Session was shared.
    Shared {
        share_id: String,
        access_level: AccessLevel,
    },
    /// Share was revoked.
    Revoked { share_id: String },
    /// New version was created.
    VersionCreated {
        version: String,
        commit_message: Option<String>,
    },
    /// Version was restored.
    VersionRestored { version: String },
    /// Message was added.
    MessageAdded { index: usize },
    /// Session was imported.
    Imported { source: String },
}

impl CollaborationEvent {
    pub fn new(session_id: &str, actor: &str, event_type: CollaborationEventType) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        session_id.hash(&mut hasher);
        now_iso().hash(&mut hasher);
        let id = format!("evt_{:x}", hasher.finish());

        Self {
            id,
            session_id: session_id.to_string(),
            actor: actor.to_string(),
            event_type,
            timestamp: now_iso(),
        }
    }
}

/// Collaboration event log for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationLog {
    /// Session ID.
    pub session_id: String,
    /// Events in chronological order.
    pub events: Vec<CollaborationEvent>,
}

impl CollaborationLog {
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            events: Vec::new(),
        }
    }

    /// Load from disk.
    pub fn load(path: &Path, session_id: &str) -> ToolResult<Self> {
        let log_path = path
            .parent()
            .ok_or_else(|| ToolError::TodoError("invalid path".into()))?
            .join("collab")
            .join(format!("{session_id}.json"));

        if !log_path.exists() {
            return Ok(Self::new(session_id));
        }

        let content = fs::read_to_string(&log_path).map_err(ToolError::Io)?;
        serde_json::from_str(&content)
            .map_err(|e| ToolError::TodoError(format!("deserialize collab log: {e}")))
    }

    /// Save to disk.
    pub fn save(&self, path: &Path) -> ToolResult<()> {
        let collab_dir = path
            .parent()
            .ok_or_else(|| ToolError::TodoError("invalid path".into()))?
            .join("collab");
        fs::create_dir_all(&collab_dir).map_err(ToolError::Io)?;

        let log_path = collab_dir.join(format!("{}.json", self.session_id));
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ToolError::TodoError(format!("serialize collab log: {e}")))?;
        fs::write(log_path, content).map_err(ToolError::Io)
    }

    /// Push an event.
    pub fn push(&mut self, event: CollaborationEvent) {
        self.events.push(event);
    }

    /// Get recent events.
    pub fn recent(&self, n: usize) -> &[CollaborationEvent] {
        let start = self.events.len().saturating_sub(n);
        &self.events[start..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_message(content: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: content.to_string(),
            created_at: now_iso(),
            estimated_tokens: 0,
            is_compressed: false,
        }
    }

    fn create_test_session(id: &str, messages: Vec<Message>) -> Session {
        Session {
            id: id.to_string(),
            name: "Test Session".to_string(),
            created_at: now_iso(),
            updated_at: now_iso(),
            message_count: messages.len() as u64,
            messages,
            metadata: HashMap::new(),
            parent_session_id: None,
            child_session_ids: Vec::new(),
        }
    }

    // ── Version History ────────────────────────────────────────────

    #[test]
    fn test_version_history_new() {
        let hist = VersionHistory::new("ses_1");
        assert_eq!(hist.session_id, "ses_1");
        assert!(hist.versions.is_empty());
        assert_eq!(hist.len(), 0);
    }

    #[test]
    fn test_version_history_snapshot() {
        let session =
            create_test_session("ses_1", vec![test_message("Hello"), test_message("World")]);
        let version = VersionHistory::snapshot(&session, Some("Initial state".to_string()));
        assert_eq!(version.session_id, "ses_1");
        assert_eq!(version.messages.len(), 2);
        assert_eq!(version.commit_message, Some("Initial state".to_string()));
    }

    #[test]
    fn test_version_history_push_and_get() {
        let session = create_test_session("ses_1", vec![test_message("msg1")]);
        let mut hist = VersionHistory::new("ses_1");
        let v1 = VersionHistory::snapshot(&session, None);
        let v1_version = v1.version.clone();
        hist.push(v1);

        assert_eq!(hist.len(), 1);
        assert!(hist.get_version(&v1_version).is_some());
        assert!(hist.get_version("nonexistent").is_none());
        assert!(hist.latest().is_some());
    }

    // ── Diff / Merge ───────────────────────────────────────────────

    #[test]
    fn test_diff_empty() {
        let msgs = vec![test_message("msg1")];
        let diff = SessionDiff::compute("ses_1", &msgs, &msgs, &HashMap::new(), &HashMap::new());
        assert!(diff.is_empty());
    }

    #[test]
    fn test_diff_append() {
        let old = vec![test_message("msg1")];
        let new = vec![test_message("msg1"), test_message("msg2")];
        let diff = SessionDiff::compute("ses_1", &old, &new, &HashMap::new(), &HashMap::new());
        assert!(!diff.is_empty());
        assert_eq!(diff.operations.len(), 1);
        match &diff.operations[0] {
            DiffOp::Insert { index, message } => {
                assert_eq!(*index, 1);
                assert_eq!(message.content, "msg2");
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_diff_delete() {
        let old = vec![test_message("msg1"), test_message("msg2")];
        let new = vec![test_message("msg1")];
        let diff = SessionDiff::compute("ses_1", &old, &new, &HashMap::new(), &HashMap::new());
        assert!(!diff.is_empty());
        assert_eq!(diff.operations.len(), 1);
        match &diff.operations[0] {
            DiffOp::Delete { index, .. } => assert_eq!(*index, 1),
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn test_diff_modify() {
        let old = vec![test_message("hello")];
        let new = vec![test_message("hi there")];
        let diff = SessionDiff::compute("ses_1", &old, &new, &HashMap::new(), &HashMap::new());
        assert!(!diff.is_empty());
        assert_eq!(diff.operations.len(), 1);
        match &diff.operations[0] {
            DiffOp::Modify {
                index, new_message, ..
            } => {
                assert_eq!(*index, 0);
                assert_eq!(new_message.content, "hi there");
            }
            _ => panic!("expected Modify"),
        }
    }

    #[test]
    fn test_diff_apply() {
        let session = create_test_session("ses_1", vec![test_message("msg1")]);
        let new_msgs = vec![test_message("msg1"), test_message("msg2")];
        let diff = SessionDiff::compute(
            "ses_1",
            &session.messages,
            &new_msgs,
            &session.metadata,
            &HashMap::new(),
        );

        let merged = diff.apply(&session).unwrap();
        assert_eq!(merged.messages.len(), 2);
        assert_eq!(merged.messages[1].content, "msg2");
    }

    #[test]
    fn test_merge_branches_append_only() {
        let base = vec![test_message("base")];
        let branch_a = vec![test_message("base"), test_message("from_a")];
        let branch_b = vec![test_message("base"), test_message("from_b")];

        let merged = merge_branches(&base, &branch_a, &branch_b);
        assert_eq!(merged.len(), 3); // base + from_a + from_b
        assert_eq!(merged[1].content, "from_a");
        assert_eq!(merged[2].content, "from_b");
    }

    #[test]
    fn test_merge_branches_no_changes() {
        let base = vec![test_message("base")];
        let merged = merge_branches(&base, &base, &base);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].content, "base");
    }

    // ── Shared Sessions ────────────────────────────────────────────

    #[test]
    fn test_shared_session_registry() {
        let mut reg = SharedSessionRegistry::new();
        let share_id = reg.share("ses_1", AccessLevel::Read, vec![], None);
        assert!(reg.shares.contains_key(&share_id));

        let lookup = reg.lookup(&share_id);
        assert!(lookup.is_some());
        assert_eq!(lookup.unwrap().session_id, "ses_1");
    }

    #[test]
    fn test_shared_session_access() {
        let mut reg = SharedSessionRegistry::new();
        let share_id = reg.share("ses_1", AccessLevel::Read, vec!["user1".to_string()], None);

        // Authorized user
        let access = reg.check_access(&share_id, "user1");
        assert_eq!(access, Some(AccessLevel::Read));

        // Unauthorized user
        let access = reg.check_access(&share_id, "user2");
        assert_eq!(access, None);
    }

    #[test]
    fn test_shared_session_expiry() {
        let mut reg = SharedSessionRegistry::new();
        // Expired in the past
        let share_id = reg.share(
            "ses_1",
            AccessLevel::Read,
            vec![],
            Some("2020-01-01T00:00:00.000Z".to_string()),
        );

        let access = reg.check_access(&share_id, "user1");
        assert_eq!(access, None); // Expired
    }

    #[test]
    fn test_shared_session_revoke() {
        let mut reg = SharedSessionRegistry::new();
        let share_id = reg.share("ses_1", AccessLevel::Write, vec![], None);
        assert!(reg.lookup(&share_id).is_some());

        let revoked = reg.revoke(&share_id);
        assert!(revoked.is_some());
        assert!(reg.lookup(&share_id).is_none());
    }

    #[test]
    fn test_shared_session_list() {
        let mut reg = SharedSessionRegistry::new();
        reg.share("ses_1", AccessLevel::Read, vec![], None);
        reg.share("ses_1", AccessLevel::Write, vec![], None);

        let shares = reg.list_for_session("ses_1");
        assert_eq!(shares.len(), 2);
    }

    // ── Branches ───────────────────────────────────────────────────

    #[test]
    fn test_session_branches_new() {
        let branches = SessionBranches::new("ses_1");
        assert_eq!(branches.active_branch, "main");
        assert!(branches.branches.contains_key("main"));
    }

    #[test]
    fn test_session_branches_create() {
        let mut branches = SessionBranches::new("ses_1");
        branches
            .create_branch("experiment", Some("testing".to_string()), "v1")
            .unwrap();
        assert!(branches.branches.contains_key("experiment"));
    }

    #[test]
    fn test_session_branches_switch() {
        let mut branches = SessionBranches::new("ses_1");
        branches.create_branch("dev", None, "v1").unwrap();
        branches.switch("dev").unwrap();
        assert_eq!(branches.active_branch, "dev");
    }

    #[test]
    fn test_session_branches_switch_nonexistent() {
        let mut branches = SessionBranches::new("ses_1");
        assert!(branches.switch("nonexistent").is_err());
    }

    #[test]
    fn test_session_branches_duplicate_name() {
        let mut branches = SessionBranches::new("ses_1");
        assert!(branches.create_branch("main", None, "v1").is_err());
    }

    // ── Collaboration Events ───────────────────────────────────────

    #[test]
    fn test_collaboration_log() {
        let mut log = CollaborationLog::new("ses_1");
        let evt = CollaborationEvent::new(
            "ses_1",
            "user1",
            CollaborationEventType::Branched {
                branch_name: "experiment".to_string(),
            },
        );
        log.push(evt);
        assert_eq!(log.events.len(), 1);
    }

    #[test]
    fn test_collaboration_log_recent() {
        let mut log = CollaborationLog::new("ses_1");
        for i in 0..5 {
            let evt = CollaborationEvent::new(
                "ses_1",
                "user1",
                CollaborationEventType::MessageAdded { index: i },
            );
            log.push(evt);
        }

        let recent = log.recent(3);
        assert_eq!(recent.len(), 3);
    }
}
