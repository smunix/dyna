//! Core data model types for the Dyna system.
//!
//! This module defines the fundamental data structures used throughout the system:
//! resources, patches, snapshots, channels, and repository metadata.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

/// A JSON resource managed by the Dyna system.
///
/// Resources are the primary unit of data. Each resource has a unique ID, a
/// human-readable name, and an arbitrary JSON body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resource {
    /// Unique identifier for this resource (e.g., "res-001").
    pub id: String,
    /// Human-readable name / title.
    pub name: String,
    /// The actual JSON content of the resource.
    pub body: serde_json::Value,
    /// Metadata associated with the resource.
    pub metadata: ResourceMetadata,
}

/// Metadata attached to a resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceMetadata {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_by: String,
    /// Monotonically increasing version counter (local).
    pub version: u64,
}

impl Resource {
    /// Create a new resource with the given ID, name, and body.
    pub fn new(id: impl Into<String>, name: impl Into<String>, body: serde_json::Value, author: impl Into<String>) -> Self {
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
// Patch / Changeset
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

/// A complete patch (changeset) in the Dyna system.
///
/// This is the fundamental unit of change, inspired by Pijul's theory of patches.
/// Each patch is self-contained, has explicit dependencies, and is identified by
/// the SHA-256 hash of its canonical serialization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Patch {
    /// Content-addressable hash of this patch (sha256:...).
    /// Computed after serialization of all other fields.
    pub hash: String,
    /// The author who created this patch.
    pub author: String,
    /// When this patch was created.
    pub timestamp: DateTime<Utc>,
    /// Human-readable description of the change.
    pub message: String,
    /// Hashes of patches that this patch depends on (Pijul-style dependencies).
    pub dependencies: Vec<String>,
    /// The resource ID that this patch targets.
    pub target_resource: String,
    /// The JSON Patch operations (RFC 6902).
    pub operations: Vec<PatchOperation>,
    /// The full "before" snapshot of the resource (for three-way merge support).
    pub parent_snapshot: Option<serde_json::Value>,
    /// The full "after" snapshot of the resource.
    pub result_snapshot: Option<serde_json::Value>,
}

/// The content of a patch used for hashing (excludes the hash field itself).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchContent {
    pub author: String,
    pub timestamp: DateTime<Utc>,
    pub message: String,
    pub dependencies: Vec<String>,
    pub target_resource: String,
    pub operations: Vec<PatchOperation>,
    pub parent_snapshot: Option<serde_json::Value>,
    pub result_snapshot: Option<serde_json::Value>,
}

impl Patch {
    /// Create a new patch and compute its content hash.
    pub fn new(
        author: String,
        message: String,
        dependencies: Vec<String>,
        target_resource: String,
        operations: Vec<PatchOperation>,
        parent_snapshot: Option<serde_json::Value>,
        result_snapshot: Option<serde_json::Value>,
    ) -> Self {
        let timestamp = Utc::now();
        let content = PatchContent {
            author: author.clone(),
            timestamp,
            message: message.clone(),
            dependencies: dependencies.clone(),
            target_resource: target_resource.clone(),
            operations: operations.clone(),
            parent_snapshot: parent_snapshot.clone(),
            result_snapshot: result_snapshot.clone(),
        };
        let serialized = serde_json::to_vec(&content).expect("Failed to serialize patch content");
        let hash = crate::hash::content_hash(&serialized);

        Self {
            hash,
            author,
            timestamp,
            message,
            dependencies,
            target_resource,
            operations,
            parent_snapshot,
            result_snapshot,
        }
    }

    /// Verify the integrity of this patch by recomputing its hash.
    pub fn verify(&self) -> bool {
        let content = PatchContent {
            author: self.author.clone(),
            timestamp: self.timestamp,
            message: self.message.clone(),
            dependencies: self.dependencies.clone(),
            target_resource: self.target_resource.clone(),
            operations: self.operations.clone(),
            parent_snapshot: self.parent_snapshot.clone(),
            result_snapshot: self.result_snapshot.clone(),
        };
        let serialized = serde_json::to_vec(&content).expect("Failed to serialize patch content");
        crate::hash::content_hash(&serialized) == self.hash
    }
}

// ---------------------------------------------------------------------------
// Channel (Pijul-inspired branch)
// ---------------------------------------------------------------------------

/// A channel represents a named sequence of patches, analogous to a branch
/// in Git or a channel in Pijul.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Channel {
    /// Name of the channel (e.g., "main", "alice-draft").
    pub name: String,
    /// Hash of the latest patch on this channel.
    pub head: Option<String>,
    /// Ordered list of patch hashes in this channel.
    pub patches: Vec<String>,
    /// When this channel was created.
    pub created_at: DateTime<Utc>,
    /// When this channel was last updated.
    pub updated_at: DateTime<Utc>,
}

impl Channel {
    /// Create a new empty channel.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            name: name.into(),
            head: None,
            patches: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Append a patch hash to this channel and update the head.
    pub fn append_patch(&mut self, hash: String) {
        self.head = Some(hash.clone());
        self.patches.push(hash);
        self.updated_at = Utc::now();
    }

    /// Get the list of patches that are in this channel but not in `other`.
    pub fn patches_since(&self, other_head: Option<&str>) -> Vec<String> {
        match other_head {
            None => self.patches.clone(),
            Some(head) => {
                if let Some(pos) = self.patches.iter().position(|h| h == head) {
                    self.patches[pos + 1..].to_vec()
                } else {
                    // If the head is not found, return all patches
                    self.patches.clone()
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Repository configuration
// ---------------------------------------------------------------------------

/// Local repository configuration, stored in `.dyna/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub remote_url: Option<String>,
    pub user: UserConfig,
}

/// User identity configuration.
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
    /// The resource ID.
    pub resource_id: String,
    /// Path to the resource file relative to the working directory.
    pub file_path: String,
    /// The previous version of the resource (None if new).
    pub previous: Option<serde_json::Value>,
    /// The new version of the resource.
    pub current: serde_json::Value,
    /// The computed operations (JSON Patch).
    pub operations: Vec<PatchOperation>,
}

// ---------------------------------------------------------------------------
// Conflict
// ---------------------------------------------------------------------------

/// Represents a conflict that needs manual resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// The resource that has the conflict.
    pub resource_id: String,
    /// Path within the JSON document where the conflict occurs.
    pub json_path: String,
    /// The local value.
    pub local_value: serde_json::Value,
    /// The remote value.
    pub remote_value: serde_json::Value,
    /// The common ancestor value (if available).
    pub base_value: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Sync state
// ---------------------------------------------------------------------------

/// Tracks synchronization state between local and remote.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    /// The hash of the last patch known to be on the remote for each channel.
    pub remote_heads: HashMap<String, String>,
    /// Hashes of patches that have been pushed to the remote.
    pub pushed_patches: Vec<String>,
}

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
            "alice".into(),
            "Test patch".into(),
            vec![],
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
    fn test_channel_operations() {
        let mut channel = Channel::new("main");
        assert!(channel.head.is_none());
        assert!(channel.patches.is_empty());

        channel.append_patch("sha256:aaa".into());
        channel.append_patch("sha256:bbb".into());
        channel.append_patch("sha256:ccc".into());

        assert_eq!(channel.head, Some("sha256:ccc".into()));
        assert_eq!(channel.patches.len(), 3);

        let since = channel.patches_since(Some("sha256:aaa"));
        assert_eq!(since, vec!["sha256:bbb", "sha256:ccc"]);

        let all = channel.patches_since(None);
        assert_eq!(all.len(), 3);
    }
}
