//! `dyna commit` command implementation.
//!
//! Creates a new Changeset from all staged changes. The changeset groups
//! one Patch per modified resource, records the author, message, and parent
//! changeset references, and appends itself to the current channel.

use anyhow::{Result, bail};
use dyna_common::models::Changeset;
use dyna_common::patch;

use crate::repository::Repository;

pub async fn execute(message: String) -> Result<()> {
    let repo = Repository::find_current()?;
    let config = repo.load_config()?;

    // Load staged changes
    let staged_changes = repo.load_staged_changes()?;
    if staged_changes.is_empty() {
        bail!("Nothing to commit. Use 'dyna add <file>' to stage changes.");
    }

    let channel_name = repo.current_channel_name()?;
    let channel = repo.load_channel(&channel_name)?;

    // Determine parent changeset(s) for this new changeset.
    // The parent is the current head of the channel (if any).
    let parents: Vec<String> = match &channel.head_change_id {
        Some(head) => vec![head.clone()],
        None => vec![],
    };

    // Build a Patch for each staged change
    let mut patches = Vec::new();
    for staged in &staged_changes {
        let p = patch::build_patch(staged);
        patches.push(p);
    }

    // Create the Changeset
    let cs = Changeset::new(
        config.user.name.clone(),
        message.clone(),
        parents,
        patches,
    );

    // Print summary of patches in this changeset
    for p in &cs.patches {
        println!(
            "  [{}] {} -> {} op(s)",
            &p.hash[7..std::cmp::min(p.hash.len(), 19)],
            p.target_resource,
            p.operations.len()
        );
    }

    // Store the changeset (also stores its patches)
    repo.store_changeset(&cs)?;

    // Update snapshots for each resource
    for staged in &staged_changes {
        repo.save_snapshot(&staged.resource_id, &staged.current)?;
    }

    // Append to the channel
    let mut channel = repo.load_channel(&channel_name)?;
    channel.append_changeset(cs.change_id.clone());
    repo.save_channel(&channel)?;

    // Set as working change
    repo.set_working_change(Some(&cs.change_id))?;

    // Clear the staging area
    repo.clear_staging()?;

    println!(
        "\nCommitted changeset {} to channel '{}': {}",
        cs.short_change_id(),
        channel_name,
        message
    );
    println!(
        "  {} patch(es), {} total operation(s)",
        cs.patches.len(),
        cs.total_operations()
    );
    println!("  change_id:   {}", cs.change_id);
    println!("  commit_hash: {}", cs.short_commit_hash());

    if !cs.parents.is_empty() {
        println!("  parent(s):   {}", cs.parents.join(", "));
    }

    Ok(())
}
