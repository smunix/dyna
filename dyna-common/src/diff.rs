//! JSON diff engine for computing patch operations between two JSON values.
//!
//! This module implements a recursive diff algorithm that produces RFC 6902-style
//! JSON Patch operations. It handles objects, arrays, and scalar values.

use crate::models::PatchOperation;
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
            // Check for removed and modified keys
            for (key, old_val) in old_map {
                let child_path = format!("{}/{}", path, escape_json_pointer(key));
                match new_map.get(key) {
                    Some(new_val) => {
                        diff_recursive(&child_path, old_val, new_val, ops);
                    }
                    None => {
                        ops.push(PatchOperation::Remove {
                            path: child_path,
                        });
                    }
                }
            }
            // Check for added keys
            for (key, new_val) in new_map {
                if !old_map.contains_key(key) {
                    let child_path = format!("{}/{}", path, escape_json_pointer(key));
                    ops.push(PatchOperation::Add {
                        path: child_path,
                        value: new_val.clone(),
                    });
                }
            }
        }
        (Value::Array(old_arr), Value::Array(new_arr)) => {
            // For arrays, we use a simple strategy: if lengths differ or elements
            // differ, replace the entire array. A more sophisticated LCS-based
            // approach could be used for finer-grained diffs.
            if old_arr.len() == new_arr.len() {
                for (i, (old_elem, new_elem)) in old_arr.iter().zip(new_arr.iter()).enumerate() {
                    let child_path = format!("{}/{}", path, i);
                    diff_recursive(&child_path, old_elem, new_elem, ops);
                }
            } else {
                // Replace the entire array
                let target_path = if path.is_empty() { "/".to_string() } else { path.to_string() };
                ops.push(PatchOperation::Replace {
                    path: target_path,
                    value: new.clone(),
                });
            }
        }
        _ => {
            // Scalar values or type changes: emit a replace operation
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
/// This is a simplified implementation that handles the core operations.
/// For production use, a full RFC 6902 implementation would be recommended.
pub fn apply_patch(doc: &mut Value, operations: &[PatchOperation]) -> Result<(), String> {
    for op in operations {
        match op {
            PatchOperation::Add { path, value } => {
                set_value(doc, path, value.clone())?;
            }
            PatchOperation::Remove { path } => {
                remove_value(doc, path)?;
            }
            PatchOperation::Replace { path, value } => {
                set_value(doc, path, value.clone())?;
            }
            PatchOperation::Move { from, path } => {
                let val = remove_value(doc, from)?;
                set_value(doc, path, val)?;
            }
            PatchOperation::Copy { from, path } => {
                let val = get_value(doc, from)
                    .ok_or_else(|| format!("Copy source not found: {}", from))?
                    .clone();
                set_value(doc, path, val)?;
            }
            PatchOperation::Test { path, value } => {
                let actual = get_value(doc, path)
                    .ok_or_else(|| format!("Test path not found: {}", path))?;
                if actual != value {
                    return Err(format!(
                        "Test failed at {}: expected {:?}, got {:?}",
                        path, value, actual
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Navigate to a JSON Pointer path and return a reference to the value.
fn get_value<'a>(doc: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() || path == "/" {
        return Some(doc);
    }
    let parts = parse_pointer(path);
    let mut current = doc;
    for part in &parts {
        match current {
            Value::Object(map) => {
                current = map.get(part.as_str())?;
            }
            Value::Array(arr) => {
                let idx: usize = part.parse().ok()?;
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

/// Set a value at a JSON Pointer path.
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

    let mut current = doc;
    for part in parent_parts {
        match current {
            Value::Object(map) => {
                current = map
                    .entry(part.clone())
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
            }
            Value::Array(arr) => {
                let idx: usize = part
                    .parse()
                    .map_err(|_| format!("Invalid array index: {}", part))?;
                if idx >= arr.len() {
                    return Err(format!("Array index out of bounds: {}", idx));
                }
                current = &mut arr[idx];
            }
            _ => return Err(format!("Cannot navigate into scalar at {}", part)),
        }
    }

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
                let idx: usize = last_key
                    .parse()
                    .map_err(|_| format!("Invalid array index: {}", last_key))?;
                if idx > arr.len() {
                    return Err(format!("Array index out of bounds: {}", idx));
                }
                arr.insert(idx, value);
                Ok(())
            }
        }
        _ => Err(format!("Cannot set value on scalar at {}", last_key)),
    }
}

/// Remove a value at a JSON Pointer path and return it.
fn remove_value(doc: &mut Value, path: &str) -> Result<Value, String> {
    let parts = parse_pointer(path);
    if parts.is_empty() {
        let old = doc.clone();
        *doc = Value::Null;
        return Ok(old);
    }

    let (parent_parts, last) = parts.split_at(parts.len() - 1);
    let last_key = &last[0];

    let mut current = doc;
    for part in parent_parts {
        match current {
            Value::Object(map) => {
                current = map
                    .get_mut(part.as_str())
                    .ok_or_else(|| format!("Path not found: {}", part))?;
            }
            Value::Array(arr) => {
                let idx: usize = part
                    .parse()
                    .map_err(|_| format!("Invalid array index: {}", part))?;
                current = arr
                    .get_mut(idx)
                    .ok_or_else(|| format!("Array index out of bounds: {}", idx))?;
            }
            _ => return Err(format!("Cannot navigate into scalar at {}", part)),
        }
    }

    match current {
        Value::Object(map) => map
            .remove(last_key.as_str())
            .ok_or_else(|| format!("Key not found: {}", last_key)),
        Value::Array(arr) => {
            let idx: usize = last_key
                .parse()
                .map_err(|_| format!("Invalid array index: {}", last_key))?;
            if idx >= arr.len() {
                return Err(format!("Array index out of bounds: {}", idx));
            }
            Ok(arr.remove(idx))
        }
        _ => Err(format!("Cannot remove from scalar at {}", last_key)),
    }
}

/// Parse a JSON Pointer string into its component parts.
fn parse_pointer(path: &str) -> Vec<String> {
    if path.is_empty() {
        return Vec::new();
    }
    let stripped = path.strip_prefix('/').unwrap_or(path);
    if stripped.is_empty() {
        return Vec::new();
    }
    stripped
        .split('/')
        .map(|s| unescape_json_pointer(s))
        .collect()
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
    // Both sides changed
    if local == remote {
        // Both made the same change
        return local.clone();
    }

    // Both sides changed differently — need to merge or conflict
    match (base, local, remote) {
        (Value::Object(base_map), Value::Object(local_map), Value::Object(remote_map)) => {
            let mut merged = serde_json::Map::new();
            let mut all_keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            all_keys.extend(base_map.keys().cloned());
            all_keys.extend(local_map.keys().cloned());
            all_keys.extend(remote_map.keys().cloned());

            for key in &all_keys {
                let child_path = if path.is_empty() {
                    format!("/{}", key)
                } else {
                    format!("{}/{}", path, key)
                };
                let base_val = base_map.get(key).unwrap_or(&Value::Null);
                let local_val = local_map.get(key).unwrap_or(&Value::Null);
                let remote_val = remote_map.get(key).unwrap_or(&Value::Null);

                let merged_val = merge_recursive(&child_path, base_val, local_val, remote_val, conflicts);
                if merged_val != Value::Null || local_map.contains_key(key) || remote_map.contains_key(key) {
                    merged.insert(key.clone(), merged_val);
                }
            }
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

        let merged = three_way_merge(&base, &local, &remote).unwrap();
        assert_eq!(merged["age"], json!(31));
        assert_eq!(merged["status"], json!("inactive"));
    }

    #[test]
    fn test_three_way_merge_conflict() {
        let base = json!({"name": "Alice", "status": "active"});
        let local = json!({"name": "Alice", "status": "approved"});
        let remote = json!({"name": "Alice", "status": "rejected"});

        let result = three_way_merge(&base, &local, &remote);
        assert!(result.is_err());
        let conflicts = result.unwrap_err();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].json_path, "/status");
    }
}
