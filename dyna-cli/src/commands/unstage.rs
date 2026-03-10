//! `dyna unstage` — move staged changes back to the working directory.
//!
//! For normal staged changes (add/modify), simply removes the staging file.
//! For staged deletions (current == null), also restores the working directory
//! file from the previous snapshot stored in the staged change.

use crate::repository::Repository;
use anyhow::Result;
use dyna_core::models::StagedChange;

/// Execute the unstage command.
///
/// - If `all` is true, unstage every staged change.
/// - Otherwise, `pattern` must be a resource ID or file path to unstage.
pub async fn execute(pattern: Option<String>, all: bool) -> Result<()> {
    let repo = Repository::find_current()?;
    let staged_changes = repo.load_staged_changes()?;

    if staged_changes.is_empty() {
        println!("Nothing staged to unstage.");
        return Ok(());
    }

    if all {
        let count = staged_changes.len();
        for staged in &staged_changes {
            unstage_one(&repo, &staged.resource_id, staged)?;
        }
        println!("Unstaged {} resource(s).", count);
        return Ok(());
    }

    let pattern = pattern.ok_or_else(|| {
        anyhow::anyhow!(
            "Please provide a resource ID or file path, or use --all to unstage everything."
        )
    })?;

    // Resolve the pattern to a resource ID
    let resource_id = if pattern.contains('/') || pattern.ends_with(".json") {
        repo.resource_id_from_relative(&pattern)
    } else {
        pattern.clone()
    };

    // Find the matching staged change
    let staged = staged_changes
        .iter()
        .find(|s| s.resource_id == resource_id)
        .ok_or_else(|| anyhow::anyhow!("Resource '{}' is not staged.", resource_id))?;

    unstage_one(&repo, &resource_id, staged)?;
    println!("Unstaged '{}'.", resource_id);
    Ok(())
}

/// Unstage a single resource.
///
/// 1. If the staged change is a deletion (current == Null), restore the
///    working directory file from the previous snapshot.
/// 2. Remove the staging file.
fn unstage_one(repo: &Repository, resource_id: &str, staged: &StagedChange) -> Result<()> {
    // For staged deletions, restore the working directory file
    if staged.current.is_null() {
        if let Some(ref previous) = staged.previous {
            let content = serde_json::to_string_pretty(previous)?;
            repo.write_resource_file(resource_id, &content)?;
            println!(
                "Restored working file for deleted resource '{}'",
                resource_id
            );
        }
    }

    // Remove the staging file
    repo.remove_staging_file(resource_id)?;
    Ok(())
}
