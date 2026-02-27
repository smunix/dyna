//! Patch building, serialization, and analysis.
//!
//! In the changeset-centric model, a [`Patch`] is a purely resource-level
//! object: it captures the JSON Patch (RFC 6902) operations applied to a
//! single resource, along with optional before/after snapshots. All metadata
//! such as author, message, and dependency information is managed by the
//! parent [`Changeset`].
//!
//! This module provides utilities for:
//! - Building patches from staged changes.
//! - Computing content-addressed hashes for patches.
//! - Analyzing commutativity between patches (Pijul-inspired).

use crate::diff;
use crate::models::{Patch, PatchOperation, StagedChange};
use itertools::{izip, Itertools};
use serde_json::Value;

/// Build a Patch from a staged change.
///
/// The patch captures the resource ID, operations, and before/after snapshots.
/// Author and message metadata are stored on the parent Changeset.
pub fn build_patch(staged: &StagedChange) -> Patch {
    let operations = staged
        .operations
        .is_empty()
        .then(|| {
            staged
                .previous
                .as_ref()
                .map(|prev| diff::diff(prev, &staged.current))
                .unwrap_or_else(|| {
                    vec![PatchOperation::Add {
                        path: "/".to_string(),
                        value: staged.current.clone(),
                    }]
                })
        })
        .unwrap_or_else(|| staged.operations.clone());

    Patch::new(
        staged.resource_id.clone(),
        operations,
        staged.previous.clone(),
        Some(staged.current.clone()),
    )
}

/// Serialize a patch to canonical JSON bytes.
pub fn serialize_patch(patch: &Patch) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec_pretty(patch)
}

/// Deserialize a patch from JSON bytes.
pub fn deserialize_patch(data: &[u8]) -> Result<Patch, serde_json::Error> {
    serde_json::from_slice(data)
}

/// Apply a patch's operations to a JSON document.
pub fn apply_patch_to_document(doc: &mut Value, patch: &Patch) -> Result<(), String> {
    diff::apply_patch(doc, &patch.operations)
}

/// Determine if two patches can commute (are independent).
///
/// Uses `cartesian_product` from itertools to check all path pairs.
pub fn patches_commute(a: &Patch, b: &Patch) -> bool {
    (a.target_resource != b.target_resource)
        || izip!(&a.operations)
            .map(op_path)
            .cartesian_product(izip!(&b.operations).map(op_path))
            .all(|(ap, bp)| !paths_overlap(ap, bp))
}

fn op_path(op: &PatchOperation) -> &str {
    match op {
        PatchOperation::Add { path, .. }
        | PatchOperation::Remove { path }
        | PatchOperation::Replace { path, .. }
        | PatchOperation::Move { path, .. }
        | PatchOperation::Copy { path, .. }
        | PatchOperation::Test { path, .. } => path,
    }
}

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

        let patch = build_patch(&staged);
        assert_eq!(patch.target_resource, "res-001");
        assert!(!patch.operations.is_empty());
        assert!(patch.verify());
    }

    #[test]
    fn test_serialize_deserialize_patch() {
        let patch = Patch::new(
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
            "res-001".into(),
            vec![PatchOperation::Replace {
                path: "/name".into(),
                value: json!("Alice"),
            }],
            None,
            None,
        );
        let b = Patch::new(
            "res-002".into(),
            vec![PatchOperation::Replace {
                path: "/name".into(),
                value: json!("Bob"),
            }],
            None,
            None,
        );
        assert!(patches_commute(&a, &b));

        let c = Patch::new(
            "res-001".into(),
            vec![PatchOperation::Replace {
                path: "/name".into(),
                value: json!("Charlie"),
            }],
            None,
            None,
        );
        assert!(!patches_commute(&a, &c));
    }
}
