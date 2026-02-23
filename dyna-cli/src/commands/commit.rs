//! `dyna commit` command implementation.

use anyhow::{Result, bail};
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
    let mut channel = repo.load_channel(&channel_name)?;

    // The dependencies for new patches are the current head of the channel
    let dependencies = match &channel.head {
        Some(head) => vec![head.clone()],
        None => vec![],
    };

    let mut created_patches = Vec::new();

    for staged in &staged_changes {
        // Build the patch
        let p = patch::build_patch(
            staged,
            &config.user.name,
            &message,
            dependencies.clone(),
        );

        // Store the patch
        repo.store_patch(&p)?;

        // Update the snapshot
        repo.save_snapshot(&staged.resource_id, &staged.current)?;

        // Append to channel
        channel.append_patch(p.hash.clone());

        println!(
            "  [{}] {} -> {}",
            &p.hash[7..19], // Show a short hash
            staged.resource_id,
            p.operations.len()
        );
        created_patches.push(p);
    }

    // Save the updated channel
    repo.save_channel(&channel)?;

    // Clear the staging area
    repo.clear_staging()?;

    println!(
        "\nCommitted {} patch(es) to channel '{}': {}",
        created_patches.len(),
        channel_name,
        message
    );

    if let Some(head) = &channel.head {
        println!("  HEAD: {}", &head[..std::cmp::min(head.len(), 19)]);
    }

    Ok(())
}
