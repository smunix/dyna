//! Core data model for the Dyna distributed CRUD system.
//!
//! This module defines the fundamental data structures for the changeset-centric
//! version control model inspired by Jujutsu:
//!
//! - [`PatchOperation`]: A single JSON Patch (RFC 6902) operation (add, remove,
//!   replace, move, copy, test).
//! - [`Patch`]: An atomic, content-addressed set of operations targeting a single
//!   JSON resource. Patches are grouped into changesets.
//! - [`Changeset`]: The primary unit of work. Groups one or more patches committed
//!   together, with a stable `change_id`, a content-derived `commit_hash`, parent
//!   references forming a DAG, and optional bookmarks. Changesets are mutable
//!   locally until promoted, at which point they become immutable.
//! - [`Channel`]: A named bookmark pointing to an ordered sequence of changeset
//!   IDs, analogous to a branch in Git or a bookmark in Jujutsu.
//! - Supporting types: [`StagedChange`], [`Conflict`], [`SyncState`], [`RepoConfig`].

use chrono::{DateTime, Utc};
use itertools::{izip, Itertools};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

/// A JSON resource managed by the Dyna system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub body: serde_json::Value,
    pub metadata: ResourceMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceMetadata {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_by: String,
    pub version: u64,
}

impl Resource {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        body: serde_json::Value,
        author: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        let author = author.into();
        Self {
            id: id.into(),
            name: name.into(),
            body,
            metadata: ResourceMetadata {
                created_at: now,
                updated_at: now,
                created_by: author.clone(),
                updated_by: author,
                version: 1,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Patch Operation (RFC 6902)
// ---------------------------------------------------------------------------

/// A single operation within a JSON Patch (RFC 6902).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum PatchOperation {
    Add {
        path: String,
        value: serde_json::Value,
    },
    Remove {
        path: String,
    },
    Replace {
        path: String,
        value: serde_json::Value,
    },
    Move {
        from: String,
        path: String,
    },
    Copy {
        from: String,
        path: String,
    },
    Test {
        path: String,
        value: serde_json::Value,
    },
}

// ---------------------------------------------------------------------------
// Patch (atomic change to a single resource)
// ---------------------------------------------------------------------------

/// A Patch is an atomic set of operations targeting a single resource.
/// Multiple patches are grouped into a Changeset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Patch {
    /// Content-addressable hash (sha256:...).
    pub hash: String,
    /// The resource this patch targets.
    pub target_resource: String,
    /// The JSON Patch operations (RFC 6902).
    pub operations: Vec<PatchOperation>,
    /// The "before" snapshot (for three-way merge).
    pub parent_snapshot: Option<serde_json::Value>,
    /// The "after" snapshot.
    pub result_snapshot: Option<serde_json::Value>,
}

/// Content used for hashing a Patch (excludes the hash field).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchContent {
    pub target_resource: String,
    pub operations: Vec<PatchOperation>,
    pub parent_snapshot: Option<serde_json::Value>,
    pub result_snapshot: Option<serde_json::Value>,
}

impl Patch {
    /// Create a new patch and compute its content hash.
    pub fn new(
        target_resource: String,
        operations: Vec<PatchOperation>,
        parent_snapshot: Option<serde_json::Value>,
        result_snapshot: Option<serde_json::Value>,
    ) -> Self {
        let content = PatchContent {
            target_resource: target_resource.clone(),
            operations: operations.clone(),
            parent_snapshot: parent_snapshot.clone(),
            result_snapshot: result_snapshot.clone(),
        };
        let serialized =
            serde_json::to_vec(&content).expect("Failed to serialize patch content");
        let hash = crate::hash::content_hash(&serialized);

        Self {
            hash,
            target_resource,
            operations,
            parent_snapshot,
            result_snapshot,
        }
    }

    /// Verify the integrity of this patch by recomputing its hash.
    pub fn verify(&self) -> bool {
        let content = PatchContent {
            target_resource: self.target_resource.clone(),
            operations: self.operations.clone(),
            parent_snapshot: self.parent_snapshot.clone(),
            result_snapshot: self.result_snapshot.clone(),
        };
        let serialized =
            serde_json::to_vec(&content).expect("Failed to serialize patch content");
        crate::hash::content_hash(&serialized) == self.hash
    }
}

// ---------------------------------------------------------------------------
// Changeset (Jujutsu-inspired)
// ---------------------------------------------------------------------------

/// A Changeset is the primary unit of work in Dyna, inspired by Jujutsu's
/// change concept.
///
/// Key properties (mirroring Jujutsu):
/// - Has an immutable **change_id** (randomly generated, stays constant).
/// - Has a mutable **commit_hash** (recomputed when content changes).
/// - Contains one or more **Patches** (grouped atomic changes).
/// - Has **parent** changesets forming a DAG.
/// - Can be mutable (working/draft) or immutable (promoted/published).
/// - Can be described with a human-readable message.
///
/// The working copy is always a Changeset (like jj's `@`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Changeset {
    /// Immutable change identifier. Stays constant even as the changeset is
    /// modified. This is a short random hex string (e.g., "a7f3bc12").
    pub change_id: String,

    /// Content-addressable commit hash (sha256:...). Recomputed whenever the
    /// changeset's content (patches, message, parents) changes.
    pub commit_hash: String,

    /// Human-readable description of this changeset.
    pub message: String,

    /// The author who created this changeset.
    pub author: String,

    /// When this changeset was created.
    pub created_at: DateTime<Utc>,

    /// When this changeset was last modified.
    pub updated_at: DateTime<Utc>,

    /// Parent changeset IDs (change_ids). Empty for the root changeset.
    /// Multiple parents indicate a merge changeset.
    pub parents: Vec<String>,

    /// The patches contained in this changeset, in order.
    pub patches: Vec<Patch>,

    /// Whether this changeset is immutable (promoted/published).
    /// Immutable changesets cannot be edited.
    pub immutable: bool,

    /// Whether this changeset is empty (no patches).
    pub empty: bool,

    /// Optional bookmark labels pointing to this changeset.
    pub bookmarks: Vec<String>,
}

/// Content used for computing the commit hash of a Changeset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetContent {
    pub change_id: String,
    pub message: String,
    pub author: String,
    pub parents: Vec<String>,
    pub patch_hashes: Vec<String>,
}

impl Changeset {
    /// Create a new changeset with a random change_id.
    pub fn new(
        author: String,
        message: String,
        parents: Vec<String>,
        patches: Vec<Patch>,
    ) -> Self {
        let change_id = generate_change_id();
        let now = Utc::now();
        let empty = patches.is_empty();
        let patch_hashes: Vec<String> = izip!(&patches).map(|p| p.hash.clone()).collect();

        let content = ChangesetContent {
            change_id: change_id.clone(),
            message: message.clone(),
            author: author.clone(),
            parents: parents.clone(),
            patch_hashes,
        };
        let serialized =
            serde_json::to_vec(&content).expect("Failed to serialize changeset content");
        let commit_hash = crate::hash::content_hash(&serialized);

        Self {
            change_id,
            commit_hash,
            message,
            author,
            created_at: now,
            updated_at: now,
            parents,
            patches,
            immutable: false,
            empty,
            bookmarks: Vec::new(),
        }
    }

    /// Recompute the commit hash after modifying the changeset.
    pub fn recompute_hash(&mut self) {
        let patch_hashes: Vec<String> = izip!(&self.patches).map(|p| p.hash.clone()).collect();
        let content = ChangesetContent {
            change_id: self.change_id.clone(),
            message: self.message.clone(),
            author: self.author.clone(),
            parents: self.parents.clone(),
            patch_hashes,
        };
        let serialized =
            serde_json::to_vec(&content).expect("Failed to serialize changeset content");
        self.commit_hash = crate::hash::content_hash(&serialized);
        self.updated_at = Utc::now();
        self.empty = self.patches.is_empty();
    }

    /// Verify the integrity of this changeset.
    pub fn verify(&self) -> bool {
        let patch_hashes: Vec<String> = izip!(&self.patches).map(|p| p.hash.clone()).collect();
        let content = ChangesetContent {
            change_id: self.change_id.clone(),
            message: self.message.clone(),
            author: self.author.clone(),
            parents: self.parents.clone(),
            patch_hashes,
        };
        let serialized =
            serde_json::to_vec(&content).expect("Failed to serialize changeset content");
        crate::hash::content_hash(&serialized) == self.commit_hash
    }

    /// Return a short display form of the change_id (first 8 chars).
    pub fn short_change_id(&self) -> &str {
        &self.change_id[..std::cmp::min(self.change_id.len(), 8)]
    }

    /// Return a short display form of the commit_hash.
    pub fn short_commit_hash(&self) -> &str {
        &self.commit_hash[7..std::cmp::min(self.commit_hash.len(), 19)]
    }

    /// Total number of operations across all patches.
    pub fn total_operations(&self) -> usize {
        izip!(&self.patches).map(|p| p.operations.len()).sum()
    }

    /// Get all resource IDs affected by this changeset.
    pub fn affected_resources(&self) -> Vec<String> {
        let mut resources: Vec<String> = izip!(&self.patches)
            .map(|p| p.target_resource.clone())
            .collect();
        resources.sort();
        resources.dedup();
        resources
    }
}

/// Generate a random 16-character hex change ID.
fn generate_change_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Mix in some randomness from the address of a stack variable
    let stack_var = 0u8;
    let addr = &stack_var as *const u8 as usize;
    let mixed = nanos ^ (addr as u128);
    format!("{:016x}", mixed & 0xFFFFFFFFFFFFFFFF)
}

// ---------------------------------------------------------------------------
// Channel / Bookmark
// ---------------------------------------------------------------------------

/// A Channel is a named bookmark pointing to a changeset lineage.
/// Channels in Dyna serve the same purpose as bookmarks in Jujutsu:
/// they label a position in the changeset DAG, primarily for
/// synchronization with the remote server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Channel {
    /// Name of the channel (e.g., "main", "alice-draft").
    pub name: String,
    /// The change_id of the changeset this channel points to.
    pub head_change_id: Option<String>,
    /// Ordered list of changeset change_ids in this channel's lineage.
    pub changesets: Vec<String>,
    /// When this channel was created.
    pub created_at: DateTime<Utc>,
    /// When this channel was last updated.
    pub updated_at: DateTime<Utc>,
}

impl Channel {
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            name: name.into(),
            head_change_id: None,
            changesets: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Append a changeset to this channel.
    pub fn append_changeset(&mut self, change_id: String) {
        self.head_change_id = Some(change_id.clone());
        self.changesets.push(change_id);
        self.updated_at = Utc::now();
    }

    /// Get changesets since a given change_id (exclusive).
    pub fn changesets_since(&self, since: Option<&str>) -> Vec<String> {
        match since {
            None => self.changesets.clone(),
            Some(id) => {
                if let Some(pos) = izip!(&self.changesets).position(|c| c == id) {
                    self.changesets[pos + 1..].to_vec()
                } else {
                    self.changesets.clone()
                }
            }
        }
    }

    // Keep backward compat: patches list derived from changesets
    // (used by protocol types that still reference Channel)
    pub fn patch_count_placeholder(&self) -> usize {
        self.changesets.len()
    }
}

// ---------------------------------------------------------------------------
// Repository configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub remote_url: Option<String>,
    pub user: UserConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    pub name: String,
    pub email: String,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            remote_url: None,
            user: UserConfig {
                name: std::env::var("USER").unwrap_or_else(|_| "unknown".into()),
                email: "unknown@example.com".into(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Staging area
// ---------------------------------------------------------------------------

/// Represents a staged change ready to be committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedChange {
    pub resource_id: String,
    pub file_path: String,
    pub previous: Option<serde_json::Value>,
    pub current: serde_json::Value,
    pub operations: Vec<PatchOperation>,
}

// ---------------------------------------------------------------------------
// Conflict
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub resource_id: String,
    pub json_path: String,
    pub local_value: serde_json::Value,
    pub remote_value: serde_json::Value,
    pub base_value: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Sync state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    /// The change_id of the last changeset known to be on the remote for each channel.
    pub remote_heads: HashMap<String, String>,
    /// Change IDs of changesets that have been pushed to the remote.
    pub pushed_changesets: Vec<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_creation() {
        let resource = Resource::new(
            "res-001",
            "Test Resource",
            serde_json::json!({"key": "value"}),
            "alice",
        );
        assert_eq!(resource.id, "res-001");
        assert_eq!(resource.metadata.version, 1);
        assert_eq!(resource.metadata.created_by, "alice");
    }

    #[test]
    fn test_patch_creation_and_verification() {
        let patch = Patch::new(
            "res-001".into(),
            vec![PatchOperation::Replace {
                path: "/name".into(),
                value: serde_json::json!("New Name"),
            }],
            None,
            None,
        );
        assert!(patch.hash.starts_with("sha256:"));
        assert!(patch.verify());
    }

    #[test]
    fn test_changeset_creation_and_verification() {
        let patch = Patch::new(
            "res-001".into(),
            vec![PatchOperation::Replace {
                path: "/name".into(),
                value: serde_json::json!("New Name"),
            }],
            None,
            None,
        );
        let cs = Changeset::new(
            "alice".into(),
            "Update resource name".into(),
            vec![],
            vec![patch],
        );
        assert_eq!(cs.change_id.len(), 16);
        assert!(cs.commit_hash.starts_with("sha256:"));
        assert!(cs.verify());
        assert!(!cs.empty);
        assert_eq!(cs.total_operations(), 1);
        assert_eq!(cs.affected_resources(), vec!["res-001"]);
    }

    #[test]
    fn test_changeset_recompute_hash() {
        let mut cs = Changeset::new("alice".into(), "Draft".into(), vec![], vec![]);
        let old_hash = cs.commit_hash.clone();
        cs.message = "Updated message".into();
        cs.recompute_hash();
        assert_ne!(cs.commit_hash, old_hash);
        assert!(cs.verify());
    }

    #[test]
    fn test_channel_operations() {
        let mut channel = Channel::new("main");
        assert!(channel.head_change_id.is_none());
        assert!(channel.changesets.is_empty());

        channel.append_changeset("aaa".into());
        channel.append_changeset("bbb".into());
        channel.append_changeset("ccc".into());

        assert_eq!(channel.head_change_id, Some("ccc".into()));
        assert_eq!(channel.changesets.len(), 3);

        let since = channel.changesets_since(Some("aaa"));
        assert_eq!(since, vec!["bbb", "ccc"]);

        let all = channel.changesets_since(None);
        assert_eq!(all.len(), 3);
    }
}
