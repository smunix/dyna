//! `dyna resolve` command implementation.

use anyhow::{Result, bail};
use dialoguer::Select;
use std::path::PathBuf;

use crate::repository::Repository;

pub async fn execute(path: PathBuf) -> Result<()> {
    let repo = Repository::find_current()?;
    let resource_id = Repository::resource_id_from_path(&path);

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
    let mut snapshot = repo
        .load_snapshot(&resource_id)?
        .unwrap_or_else(|| serde_json::json!({}));

    for (i, conflict) in conflicts.iter().enumerate() {
        println!(
            "--- Conflict {} of {} ---",
            i + 1,
            conflicts.len()
        );
        println!("  Path: {}", conflict.json_path);
        println!(
            "  Local value:  {}",
            serde_json::to_string_pretty(&conflict.local_value)?
        );
        println!(
            "  Remote value: {}",
            serde_json::to_string_pretty(&conflict.remote_value)?
        );
        if let Some(base) = &conflict.base_value {
            println!(
                "  Base value:   {}",
                serde_json::to_string_pretty(base)?
            );
        }
        println!();

        let choices = vec![
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
            0 => conflict.local_value.clone(),
            1 => conflict.remote_value.clone(),
            2 => {
                println!("Enter the resolved JSON value:");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                serde_json::from_str(input.trim())
                    .map_err(|e| anyhow::anyhow!("Invalid JSON: {}", e))?
            }
            _ => unreachable!(),
        };

        // Apply the resolution to the snapshot
        let path_parts: Vec<&str> = conflict
            .json_path
            .trim_start_matches('/')
            .split('/')
            .collect();

        set_nested_value(&mut snapshot, &path_parts, resolved_value)?;

        println!("  Resolved.\n");
    }

    // Save the resolved snapshot
    repo.save_snapshot(&resource_id, &snapshot)?;

    // Write to working directory
    let resource_path = repo.work_dir.join(format!("{}.json", resource_id));
    let json = serde_json::to_string_pretty(&snapshot)?;
    std::fs::write(&resource_path, &json)?;

    // Clear conflicts
    repo.clear_conflicts(&resource_id)?;

    // Auto-stage the resolution
    let operations = dyna_core::diff::diff(
        &conflicts.first().map(|c| c.local_value.clone()).unwrap_or_default(),
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
fn set_nested_value(
    doc: &mut serde_json::Value,
    path: &[&str],
    value: serde_json::Value,
) -> Result<()> {
    if path.is_empty() {
        *doc = value;
        return Ok(());
    }

    let mut current = doc;
    for (i, key) in path.iter().enumerate() {
        if i == path.len() - 1 {
            // Last key: set the value
            match current {
                serde_json::Value::Object(map) => {
                    map.insert(key.to_string(), value);
                    return Ok(());
                }
                serde_json::Value::Array(arr) => {
                    let idx: usize = key.parse()
                        .map_err(|_| anyhow::anyhow!("Invalid array index: {}", key))?;
                    if idx < arr.len() {
                        arr[idx] = value;
                    } else {
                        arr.push(value);
                    }
                    return Ok(());
                }
                _ => bail!("Cannot set value at path: not an object or array"),
            }
        } else {
            // Navigate deeper
            match current {
                serde_json::Value::Object(map) => {
                    current = map
                        .entry(key.to_string())
                        .or_insert_with(|| serde_json::json!({}));
                }
                serde_json::Value::Array(arr) => {
                    let idx: usize = key.parse()
                        .map_err(|_| anyhow::anyhow!("Invalid array index: {}", key))?;
                    current = &mut arr[idx];
                }
                _ => bail!("Cannot navigate into non-object/array at '{}'", key),
            }
        }
    }

    Ok(())
}
