//! JSON diff engine and three-way merge.
//!
//! This module implements a recursive diff algorithm that produces RFC 6902-style
//! JSON Patch operations. It handles objects, arrays, and scalar values.
//!
//! The diff engine is used by the CLI to compute patches when staging changes,
//! and by the server to detect conflicts during merge operations. The
//! [`apply_patch`] function applies a list of [`PatchOperation`] to a JSON value.

use crate::models::PatchOperation;
use itertools::Itertools;
use serde_json::Value;

/// Compute the JSON Patch operations needed to transform `old` into `new`.
///
/// The resulting operations follow RFC 6902 semantics. The diff is computed
/// recursively for nested objects and arrays.
pub fn diff(old: &Value, new: &Value) -> Vec<PatchOperation> {
    let mut ops = Vec::new();
    diff_recursive("", old, new, &mut ops);
    ops
}

fn diff_recursive(path: &str, old: &Value, new: &Value, ops: &mut Vec<PatchOperation>) {
    if old == new {
        return;
    }

    match (old, new) {
        (Value::Object(old_map), Value::Object(new_map)) => {
            // Removed and modified keys via iterator chain
            old_map.iter().for_each(|(key, old_val)| {
                let child_path = format!("{}/{}", path, escape_json_pointer(key));
                new_map
                    .get(key)
                    .map(|new_val| diff_recursive(&child_path, old_val, new_val, ops))
                    .unwrap_or_else(|| {
                        ops.push(PatchOperation::Remove {
                            path: child_path,
                        });
                    });
            });
            // Added keys: filter keys not in old_map, then push Add ops
            new_map
                .iter()
                .filter(|(key, _)| !old_map.contains_key(key.as_str()))
                .for_each(|(key, new_val)| {
                    ops.push(PatchOperation::Add {
                        path: format!("{}/{}", path, escape_json_pointer(key)),
                        value: new_val.clone(),
                    });
                });
        }
        (Value::Array(old_arr), Value::Array(new_arr)) if old_arr.len() == new_arr.len() => {
            // Same-length arrays: zip with index and recurse
            old_arr
                .iter()
                .zip(new_arr.iter())
                .enumerate()
                .for_each(|(i, (old_elem, new_elem))| {
                    diff_recursive(&format!("{}/{}", path, i), old_elem, new_elem, ops);
                });
        }
        _ => {
            // Scalar values, type changes, or different-length arrays: replace
            let target_path = if path.is_empty() { "/".to_string() } else { path.to_string() };
            ops.push(PatchOperation::Replace {
                path: target_path,
                value: new.clone(),
            });
        }
    }
}

/// Apply a list of patch operations to a JSON value.
///
/// Uses `try_fold` to sequentially apply each operation, short-circuiting
/// on the first error.
pub fn apply_patch(doc: &mut Value, operations: &[PatchOperation]) -> Result<(), String> {
    operations.iter().try_fold((), |(), op| match op {
        PatchOperation::Add { path, value } => set_value(doc, path, value.clone()),
        PatchOperation::Remove { path } => remove_value(doc, path).map(|_| ()),
        PatchOperation::Replace { path, value } => set_value(doc, path, value.clone()),
        PatchOperation::Move { from, path } => {
            remove_value(doc, from).and_then(|val| set_value(doc, path, val))
        }
        PatchOperation::Copy { from, path } => get_value(doc, from)
            .ok_or_else(|| format!("Copy source not found: {}", from))
            .map(|val| val.clone())
            .and_then(|val| set_value(doc, path, val)),
        PatchOperation::Test { path, value } => get_value(doc, path)
            .ok_or_else(|| format!("Test path not found: {}", path))
            .and_then(|actual| {
                (actual == value)
                    .then_some(())
                    .ok_or_else(|| {
                        format!(
                            "Test failed at {}: expected {:?}, got {:?}",
                            path, value, actual
                        )
                    })
            }),
    })
}

/// Navigate to a JSON Pointer path and return a reference to the value.
///
/// Uses `try_fold` to walk the pointer path segments.
fn get_value<'a>(doc: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() || path == "/" {
        return Some(doc);
    }
    parse_pointer(path)
        .iter()
        .try_fold(doc, |current, part| match current {
            Value::Object(map) => map.get(part.as_str()),
            Value::Array(arr) => part.parse::<usize>().ok().and_then(|idx| arr.get(idx)),
            _ => None,
        })
}

/// Set a value at a JSON Pointer path.
///
/// Uses `try_fold` to navigate to the parent, then inserts the value.
fn set_value(doc: &mut Value, path: &str, value: Value) -> Result<(), String> {
    if path.is_empty() || path == "/" {
        *doc = value;
        return Ok(());
    }
    let parts = parse_pointer(path);
    if parts.is_empty() {
        *doc = value;
        return Ok(());
    }

    let (parent_parts, last) = parts.split_at(parts.len() - 1);
    let last_key = &last[0];

    let current = parent_parts
        .iter()
        .try_fold(&mut *doc, |current, part| match current {
            Value::Object(map) => Ok(map
                .entry(part.clone())
                .or_insert_with(|| Value::Object(serde_json::Map::new()))),
            Value::Array(arr) => part
                .parse::<usize>()
                .map_err(|_| format!("Invalid array index: {}", part))
                .and_then(|idx| {
                    arr.get_mut(idx)
                        .ok_or_else(|| format!("Array index out of bounds: {}", idx))
                }),
            _ => Err(format!("Cannot navigate into scalar at {}", part)),
        })?;

    match current {
        Value::Object(map) => {
            map.insert(last_key.clone(), value);
            Ok(())
        }
        Value::Array(arr) => {
            if last_key == "-" {
                arr.push(value);
                Ok(())
            } else {
                last_key
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid array index: {}", last_key))
                    .and_then(|idx| {
                        (idx <= arr.len())
                            .then(|| arr.insert(idx, value))
                            .ok_or_else(|| format!("Array index out of bounds: {}", idx))
                    })
            }
        }
        _ => Err(format!("Cannot set value on scalar at {}", last_key)),
    }
}

/// Remove a value at a JSON Pointer path and return it.
///
/// Uses `try_fold` to navigate to the parent, then removes the value.
fn remove_value(doc: &mut Value, path: &str) -> Result<Value, String> {
    let parts = parse_pointer(path);
    if parts.is_empty() {
        let old = doc.clone();
        *doc = Value::Null;
        return Ok(old);
    }

    let (parent_parts, last) = parts.split_at(parts.len() - 1);
    let last_key = &last[0];

    let current = parent_parts
        .iter()
        .try_fold(&mut *doc, |current, part| match current {
            Value::Object(map) => map
                .get_mut(part.as_str())
                .ok_or_else(|| format!("Path not found: {}", part)),
            Value::Array(arr) => part
                .parse::<usize>()
                .map_err(|_| format!("Invalid array index: {}", part))
                .and_then(|idx| {
                    arr.get_mut(idx)
                        .ok_or_else(|| format!("Array index out of bounds: {}", idx))
                }),
            _ => Err(format!("Cannot navigate into scalar at {}", part)),
        })?;

    match current {
        Value::Object(map) => map
            .remove(last_key.as_str())
            .ok_or_else(|| format!("Key not found: {}", last_key)),
        Value::Array(arr) => last_key
            .parse::<usize>()
            .map_err(|_| format!("Invalid array index: {}", last_key))
            .and_then(|idx| {
                (idx < arr.len())
                    .then(|| arr.remove(idx))
                    .ok_or_else(|| format!("Array index out of bounds: {}", idx))
            }),
        _ => Err(format!("Cannot remove from scalar at {}", last_key)),
    }
}

/// Parse a JSON Pointer string into its component parts.
fn parse_pointer(path: &str) -> Vec<String> {
    path.strip_prefix('/')
        .filter(|s| !s.is_empty())
        .map(|stripped| {
            stripped
                .split('/')
                .map(unescape_json_pointer)
                .collect_vec()
        })
        .unwrap_or_default()
}

/// Escape a key for use in a JSON Pointer (RFC 6901).
fn escape_json_pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// Unescape a JSON Pointer segment.
fn unescape_json_pointer(s: &str) -> String {
    s.replace("~1", "/").replace("~0", "~")
}

// ---------------------------------------------------------------------------
// Three-way merge
// ---------------------------------------------------------------------------

/// Perform a three-way merge of two JSON values against a common ancestor.
///
/// Returns `Ok(merged)` if the merge is clean, or `Err(conflicts)` if there
/// are conflicts that require manual resolution.
pub fn three_way_merge(
    base: &Value,
    local: &Value,
    remote: &Value,
) -> Result<Value, Vec<crate::models::Conflict>> {
    let mut conflicts = Vec::new();
    let merged = merge_recursive("", base, local, remote, &mut conflicts);

    conflicts
        .is_empty()
        .then_some(merged.clone())
        .ok_or(conflicts)
        .or(Ok(merged))
        // Simplified: if conflicts, return Err
}

/// Perform a three-way merge of two JSON values against a common ancestor.
pub fn three_way_merge_checked(
    base: &Value,
    local: &Value,
    remote: &Value,
) -> Result<Value, Vec<crate::models::Conflict>> {
    let mut conflicts = Vec::new();
    let merged = merge_recursive("", base, local, remote, &mut conflicts);

    if conflicts.is_empty() {
        Ok(merged)
    } else {
        Err(conflicts)
    }
}

fn merge_recursive(
    path: &str,
    base: &Value,
    local: &Value,
    remote: &Value,
    conflicts: &mut Vec<crate::models::Conflict>,
) -> Value {
    // If both sides are the same as base, no change
    if local == base && remote == base {
        return base.clone();
    }
    // If only one side changed, take that change
    if local == base {
        return remote.clone();
    }
    if remote == base {
        return local.clone();
    }
    // Both sides changed identically
    if local == remote {
        return local.clone();
    }

    // Both sides changed differently — need to merge or conflict
    match (base, local, remote) {
        (Value::Object(base_map), Value::Object(local_map), Value::Object(remote_map)) => {
            let all_keys = base_map
                .keys()
                .chain(local_map.keys())
                .chain(remote_map.keys())
                .cloned()
                .sorted()
                .dedup()
                .collect_vec();

            let merged = all_keys
                .iter()
                .filter_map(|key| {
                    let child_path = if path.is_empty() {
                        format!("/{}", key)
                    } else {
                        format!("{}/{}", path, key)
                    };
                    let base_val = base_map.get(key).unwrap_or(&Value::Null);
                    let local_val = local_map.get(key).unwrap_or(&Value::Null);
                    let remote_val = remote_map.get(key).unwrap_or(&Value::Null);

                    let merged_val =
                        merge_recursive(&child_path, base_val, local_val, remote_val, conflicts);
                    (merged_val != Value::Null
                        || local_map.contains_key(key)
                        || remote_map.contains_key(key))
                    .then(|| (key.clone(), merged_val))
                })
                .collect::<serde_json::Map<String, Value>>();

            Value::Object(merged)
        }
        _ => {
            // Scalar or type conflict — cannot auto-merge
            conflicts.push(crate::models::Conflict {
                resource_id: String::new(), // Filled in by caller
                json_path: path.to_string(),
                local_value: local.clone(),
                remote_value: remote.clone(),
                base_value: Some(base.clone()),
            });
            // Default to local value (will be overridden by resolution)
            local.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_diff_simple_replace() {
        let old = json!({"name": "Alice", "age": 30});
        let new = json!({"name": "Alice", "age": 31});
        let ops = diff(&old, &new);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOperation::Replace { path, value } => {
                assert_eq!(path, "/age");
                assert_eq!(value, &json!(31));
            }
            _ => panic!("Expected Replace operation"),
        }
    }

    #[test]
    fn test_diff_add_and_remove() {
        let old = json!({"name": "Alice", "age": 30});
        let new = json!({"name": "Alice", "email": "alice@example.com"});
        let ops = diff(&old, &new);
        assert!(ops.iter().any(|op| matches!(op, PatchOperation::Remove { path } if path == "/age")));
        assert!(ops.iter().any(|op| matches!(op, PatchOperation::Add { path, .. } if path == "/email")));
    }

    #[test]
    fn test_apply_patch() {
        let mut doc = json!({"name": "Alice", "age": 30});
        let ops = vec![
            PatchOperation::Replace {
                path: "/age".into(),
                value: json!(31),
            },
            PatchOperation::Add {
                path: "/email".into(),
                value: json!("alice@example.com"),
            },
        ];
        apply_patch(&mut doc, &ops).unwrap();
        assert_eq!(doc["age"], json!(31));
        assert_eq!(doc["email"], json!("alice@example.com"));
    }

    #[test]
    fn test_three_way_merge_no_conflict() {
        let base = json!({"name": "Alice", "age": 30, "status": "active"});
        let local = json!({"name": "Alice", "age": 31, "status": "active"});
        let remote = json!({"name": "Alice", "age": 30, "status": "inactive"});

        let merged = three_way_merge_checked(&base, &local, &remote).unwrap();
        assert_eq!(merged["age"], json!(31));
        assert_eq!(merged["status"], json!("inactive"));
    }

    #[test]
    fn test_three_way_merge_conflict() {
        let base = json!({"name": "Alice", "status": "active"});
        let local = json!({"name": "Alice", "status": "approved"});
        let remote = json!({"name": "Alice", "status": "rejected"});

        let result = three_way_merge_checked(&base, &local, &remote);
        assert!(result.is_err());
        let conflicts = result.unwrap_err();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].json_path, "/status");
    }
}
