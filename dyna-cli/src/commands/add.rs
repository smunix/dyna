//! `dyna add` command implementation.

use anyhow::{Context, Result, bail};
use dyna_common::diff;
use dyna_common::models::StagedChange;
use std::path::PathBuf;

use crate::repository::Repository;

pub async fn execute(path: PathBuf) -> Result<()> {
    let repo = Repository::find_current()?;

    // Resolve the path relative to the working directory
    let abs_path = if path.is_absolute() {
        path.clone()
    } else {
        std::env::current_dir()?.join(&path)
    };

    if !abs_path.exists() {
        bail!("File not found: {}", abs_path.display());
    }

    // Read and validate JSON
    let content = std::fs::read_to_string(&abs_path)
        .context("Failed to read file")?;
    let current: serde_json::Value = serde_json::from_str(&content)
        .context("File is not valid JSON")?;

    // Derive resource ID from filename
    let resource_id = Repository::resource_id_from_path(&abs_path);

    // Load the previous snapshot (if any)
    let previous = repo.load_snapshot(&resource_id)?;

    // Compute diff operations
    let operations = match &previous {
        Some(prev) => diff::diff(prev, &current),
        None => vec![dyna_common::models::PatchOperation::Add {
            path: "/".to_string(),
            value: current.clone(),
        }],
    };

    if operations.is_empty() {
        println!("No changes detected in {}", path.display());
        return Ok(());
    }

    // Create the staged change
    let relative_path = path.display().to_string();
    let staged = StagedChange {
        resource_id: resource_id.clone(),
        file_path: relative_path.clone(),
        previous,
        current,
        operations,
    };

    repo.stage_change(&staged)?;

    println!("Staged: {} (resource: {})", relative_path, resource_id);
    println!(
        "  {} operation(s) detected",
        staged.operations.len()
    );

    Ok(())
}
