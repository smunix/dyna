//! Patch engine for creating, serializing, and managing patches.
//!
//! This module provides the logic for building patches from staged changes,
//! serializing them for storage, and checking dependency relationships.

use crate::diff;
use crate::models::{Patch, PatchOperation, StagedChange};
use serde_json::Value;

/// Build a Patch from a staged change.
pub fn build_patch(
    staged: &StagedChange,
    author: &str,
    message: &str,
    dependencies: Vec<String>,
) -> Patch {
    let operations = if staged.operations.is_empty() {
        // Compute operations from previous and current if not already computed
        match &staged.previous {
            Some(prev) => diff::diff(prev, &staged.current),
            None => {
                // New resource: add the entire body
                vec![PatchOperation::Add {
                    path: "/".to_string(),
                    value: staged.current.clone(),
                }]
            }
        }
    } else {
        staged.operations.clone()
    };

    Patch::new(
        author.to_string(),
        message.to_string(),
        dependencies,
        staged.resource_id.clone(),
        operations,
        staged.previous.clone(),
        Some(staged.current.clone()),
    )
}

/// Serialize a patch to canonical JSON bytes (for storage and hashing).
pub fn serialize_patch(patch: &Patch) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec_pretty(patch)
}

/// Deserialize a patch from JSON bytes.
pub fn deserialize_patch(data: &[u8]) -> Result<Patch, serde_json::Error> {
    serde_json::from_slice(data)
}

/// Check if a patch's dependencies are all satisfied by the given set of
/// available patch hashes.
pub fn check_dependencies(patch: &Patch, available: &[String]) -> Vec<String> {
    patch
        .dependencies
        .iter()
        .filter(|dep| !available.contains(dep))
        .cloned()
        .collect()
}

/// Apply a patch's operations to a JSON document.
pub fn apply_patch_to_document(doc: &mut Value, patch: &Patch) -> Result<(), String> {
    diff::apply_patch(doc, &patch.operations)
}

/// Determine if two patches can commute (are independent).
///
/// Two patches commute if they target different resources, or if they
/// modify non-overlapping paths within the same resource.
pub fn patches_commute(a: &Patch, b: &Patch) -> bool {
    // Different resources always commute
    if a.target_resource != b.target_resource {
        return true;
    }

    // Same resource: check if paths overlap
    let a_paths: Vec<&str> = a.operations.iter().map(|op| op_path(op)).collect();
    let b_paths: Vec<&str> = b.operations.iter().map(|op| op_path(op)).collect();

    for ap in &a_paths {
        for bp in &b_paths {
            if paths_overlap(ap, bp) {
                return false;
            }
        }
    }
    true
}

/// Extract the path from a patch operation.
fn op_path(op: &PatchOperation) -> &str {
    match op {
        PatchOperation::Add { path, .. } => path,
        PatchOperation::Remove { path } => path,
        PatchOperation::Replace { path, .. } => path,
        PatchOperation::Move { path, .. } => path,
        PatchOperation::Copy { path, .. } => path,
        PatchOperation::Test { path, .. } => path,
    }
}

/// Check if two JSON Pointer paths overlap (one is a prefix of the other).
fn paths_overlap(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{}/", b)) || b.starts_with(&format!("{}/", a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::StagedChange;
    use serde_json::json;

    #[test]
    fn test_build_patch_from_staged_change() {
        let staged = StagedChange {
            resource_id: "res-001".into(),
            file_path: "resources/res-001.json".into(),
            previous: Some(json!({"name": "Alice", "age": 30})),
            current: json!({"name": "Alice", "age": 31}),
            operations: vec![],
        };

        let patch = build_patch(&staged, "alice", "Update age", vec![]);
        assert_eq!(patch.target_resource, "res-001");
        assert_eq!(patch.author, "alice");
        assert!(!patch.operations.is_empty());
        assert!(patch.verify());
    }

    #[test]
    fn test_serialize_deserialize_patch() {
        let patch = Patch::new(
            "alice".into(),
            "Test".into(),
            vec![],
            "res-001".into(),
            vec![PatchOperation::Replace {
                path: "/name".into(),
                value: json!("Bob"),
            }],
            None,
            None,
        );

        let bytes = serialize_patch(&patch).unwrap();
        let deserialized = deserialize_patch(&bytes).unwrap();
        assert_eq!(patch.hash, deserialized.hash);
        assert_eq!(patch.operations, deserialized.operations);
    }

    #[test]
    fn test_patches_commute() {
        let a = Patch::new(
            "alice".into(),
            "A".into(),
            vec![],
            "res-001".into(),
            vec![PatchOperation::Replace {
                path: "/name".into(),
                value: json!("Alice"),
            }],
            None,
            None,
        );
        let b = Patch::new(
            "bob".into(),
            "B".into(),
            vec![],
            "res-002".into(),
            vec![PatchOperation::Replace {
                path: "/name".into(),
                value: json!("Bob"),
            }],
            None,
            None,
        );
        // Different resources: should commute
        assert!(patches_commute(&a, &b));

        let c = Patch::new(
            "charlie".into(),
            "C".into(),
            vec![],
            "res-001".into(),
            vec![PatchOperation::Replace {
                path: "/name".into(),
                value: json!("Charlie"),
            }],
            None,
            None,
        );
        // Same resource, same path: should NOT commute
        assert!(!patches_commute(&a, &c));
    }

    #[test]
    fn test_check_dependencies() {
        let patch = Patch::new(
            "alice".into(),
            "Test".into(),
            vec!["sha256:aaa".into(), "sha256:bbb".into()],
            "res-001".into(),
            vec![],
            None,
            None,
        );

        let available = vec!["sha256:aaa".into()];
        let missing = check_dependencies(&patch, &available);
        assert_eq!(missing, vec!["sha256:bbb"]);
    }
}
