//! `dyna resolve` command implementation.
//!
//! Interactively resolves conflicts for a given resource file, then
//! auto-stages the resolution.

use anyhow::{Result, bail};
use itertools::izip;
use dialoguer::Select;
use std::path::PathBuf;

use crate::repository::Repository;

pub async fn execute(path: PathBuf) -> Result<()> {
    let repo = Repository::find_current()?;
    let resource_id = repo.resource_id_from_path(&path);

    let conflicts = repo.load_conflicts(&resource_id)?;
    if conflicts.is_empty() {
        println!("No conflicts found for '{}'.", resource_id);
        return Ok(());
    }

    println!(
        "Resolving {} conflict(s) for resource '{}':\n",
        conflicts.len(),
        resource_id
    );

    // Load current snapshot
    let base_snapshot = repo
        .load_snapshot(&resource_id)?
        .unwrap_or_else(|| serde_json::json!({}));

    // Resolve each conflict via try_fold, accumulating the resolved snapshot
    let snapshot = izip!(&conflicts)
        .enumerate()
        .try_fold(base_snapshot, |mut snapshot, (i, conflict)| -> Result<serde_json::Value> {
            println!("--- Conflict {} of {} ---", i + 1, conflicts.len());
            println!("  Path: {}", conflict.json_path);
            println!(
                "  Local value:  {}",
                serde_json::to_string_pretty(&conflict.local_value)?
            );
            println!(
                "  Remote value: {}",
                serde_json::to_string_pretty(&conflict.remote_value)?
            );
            conflict.base_value.as_ref().map(|base| {
                serde_json::to_string_pretty(base)
                    .map(|s| println!("  Base value:   {}", s))
            }).transpose()?;
            println!();

            let choices = [
                format!("Keep local:  {}", serde_json::to_string(&conflict.local_value)?),
                format!("Keep remote: {}", serde_json::to_string(&conflict.remote_value)?),
                "Enter custom value".to_string(),
            ];

            let selection = Select::new()
                .with_prompt("Choose resolution")
                .items(&choices)
                .default(0)
                .interact()?;

            let resolved_value = match selection {
                0 => Ok(conflict.local_value.clone()),
                1 => Ok(conflict.remote_value.clone()),
                2 => {
                    println!("Enter the resolved JSON value:");
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;
                    serde_json::from_str(input.trim())
                        .map_err(|e| anyhow::anyhow!("Invalid JSON: {}", e))
                }
                _ => unreachable!(),
            }?;

            let path_parts: Vec<&str> = conflict
                .json_path
                .trim_start_matches('/')
                .split('/')
                .collect();

            set_nested_value(&mut snapshot, &path_parts, resolved_value)?;
            println!("  Resolved.\n");

            Ok(snapshot)
        })?;

    // Save the resolved snapshot
    repo.save_snapshot(&resource_id, &snapshot)?;

    // Write to working directory
    serde_json::to_string_pretty(&snapshot)
        .map_err(anyhow::Error::from)
        .and_then(|json| {
            std::fs::write(
                repo.work_dir.join(format!("{}.json", resource_id)),
                &json,
            )
            .map_err(anyhow::Error::from)
        })?;

    // Clear conflicts
    repo.clear_conflicts(&resource_id)?;

    // Auto-stage the resolution
    let operations = dyna_core::diff::diff(
        &conflicts
            .first()
            .map(|c| c.local_value.clone())
            .unwrap_or_default(),
        &snapshot,
    );
    let staged = dyna_core::models::StagedChange {
        resource_id: resource_id.clone(),
        file_path: path.display().to_string(),
        previous: repo.load_snapshot(&resource_id).ok().flatten(),
        current: snapshot,
        operations,
    };
    repo.stage_change(&staged)?;

    println!(
        "All conflicts resolved for '{}'. Changes have been staged.",
        resource_id
    );
    println!("Run 'dyna commit -m \"Resolve conflicts\"' to record the resolution.");

    Ok(())
}

/// Set a value at a nested path within a JSON object.
/// Uses fold-style navigation to reach the target, then sets the value.
fn set_nested_value(
    doc: &mut serde_json::Value,
    path: &[&str],
    value: serde_json::Value,
) -> Result<()> {
    if path.is_empty() {
        *doc = value;
        return Ok(());
    }

    // Navigate to the parent of the target, then set the final key
    let (last_key, parent_path) = path
        .split_last()
        .ok_or_else(|| anyhow::anyhow!("Empty path"))?;

    let target = izip!(parent_path)
        .try_fold(doc as &mut serde_json::Value, |current, key| -> Result<&mut serde_json::Value> {
            match current {
                serde_json::Value::Object(map) => Ok(map
                    .entry((*key).to_string())
                    .or_insert_with(|| serde_json::json!({}))),
                serde_json::Value::Array(arr) => {
                    let idx: usize = key
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Invalid array index: {}", key))?;
                    arr.get_mut(idx)
                        .ok_or_else(|| anyhow::anyhow!("Array index {} out of bounds", idx))
                }
                _ => bail!("Cannot navigate into non-object/array at '{}'", key),
            }
        })?;

    match target {
        serde_json::Value::Object(map) => {
            map.insert((*last_key).to_string(), value);
            Ok(())
        }
        serde_json::Value::Array(arr) => {
            let idx: usize = last_key
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid array index: {}", last_key))?;
            if idx < arr.len() {
                arr[idx] = value;
            } else {
                arr.push(value);
            }
            Ok(())
        }
        _ => bail!("Cannot set value at path: not an object or array"),
    }
}
