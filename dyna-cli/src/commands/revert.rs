//! `dyna revert` command implementation.
//!
//! Reverts a given changeset in the current (or specified) channel by creating
//! a new changeset whose patches are the **inverse** of the original. The
//! inverse operations are computed via [`dyna_core::diff::invert_operations`].
//!
//! The revert changeset records the reverted changeset as context in its
//! commit message and sets the current channel head as its parent.

use anyhow::{Result, bail};
use colored::Colorize;
use dyna_core::diff::invert_operations;
use dyna_core::models::{Changeset, Patch, PatchContent};
use itertools::izip;

use crate::repository::Repository;

pub async fn execute(change_id: String, channel: Option<String>) -> Result<()> {
    let repo = Repository::find_current()?;
    let config = repo.load_config()?;

    let channel_name = channel
        .map(Ok)
        .unwrap_or_else(|| repo.current_channel_name())?;

    // Protect the main channel
    if channel_name == "main" {
        bail!(
            "Cannot revert directly on the 'main' channel. \
             Switch to a feature channel first, then promote to main."
        );
    }

    // Resolve the changeset (supports prefix matching)
    let matches = repo.find_changeset_by_prefix(&change_id)?;
    let target_cs = match matches.len() {
        0 => bail!("No changeset found matching '{}'", change_id),
        1 => matches.into_iter().next().unwrap(),
        n => bail!(
            "Ambiguous prefix '{}' matches {} changesets. Please provide a longer prefix.",
            change_id, n
        ),
    };

    println!(
        "Reverting changeset {} ({})...",
        target_cs.short_change_id().bold(),
        target_cs.message
    );

    if target_cs.patches.is_empty() {
        bail!("Changeset {} has no patches to revert.", target_cs.short_change_id());
    }

    // Build inverse patches for each patch in the changeset.
    // We use the patch's parent_snapshot as the base for inversion (the state
    // before the patch was applied).
    let inverse_patches: Vec<Patch> = izip!(&target_cs.patches)
        .map(|original| {
            let base = original
                .parent_snapshot
                .as_ref()
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            let inverse_ops = invert_operations(&original.operations, &base);

            // Build the inverse patch with swapped snapshots
            let content = PatchContent {
                target_resource: original.target_resource.clone(),
                operations: inverse_ops,
                parent_snapshot: original.result_snapshot.clone(),
                result_snapshot: original.parent_snapshot.clone(),
            };
            let serialized =
                serde_json::to_vec(&content).expect("Failed to serialize patch content");
            let hash = dyna_core::hash::content_hash(&serialized);

            Patch {
                hash,
                target_resource: content.target_resource,
                operations: content.operations,
                parent_snapshot: content.parent_snapshot,
                result_snapshot: content.result_snapshot,
            }
        })
        .collect();

    // Parent is the current channel head
    let ch = repo.load_channel(&channel_name)?;
    let parents = ch
        .head_change_id
        .clone()
        .map(|id| vec![id])
        .unwrap_or_default();

    let revert_message = format!("Revert \"{}\" ({})", target_cs.message, target_cs.short_change_id());

    let revert_cs = Changeset::new(
        config.user.name.clone(),
        revert_message.clone(),
        parents,
        inverse_patches,
    );

    // Print summary
    izip!(&revert_cs.patches).for_each(|p| {
        println!(
            "  [{}] {} -> {} op(s)",
            &p.hash[7..std::cmp::min(p.hash.len(), 19)],
            p.target_resource,
            p.operations.len()
        );
    });

    // Store the revert changeset
    repo.store_changeset(&revert_cs)?;

    // Update snapshots: apply the inverse patches to current snapshots
    izip!(&revert_cs.patches).try_for_each(|p| {
        p.result_snapshot
            .as_ref()
            .map(|snap| {
                if snap.is_null() {
                    repo.remove_snapshot(&p.target_resource)
                } else {
                    repo.save_snapshot(&p.target_resource, snap)
                }
            })
            .unwrap_or(Ok(()))
    })?;

    // Append to the channel
    repo.load_channel(&channel_name)
        .and_then(|mut channel| {
            channel.append_changeset(revert_cs.change_id.clone());
            repo.save_channel(&channel)
        })?;

    // Set as working change
    repo.set_working_change(Some(&revert_cs.change_id))?;

    println!(
        "\n{} Reverted changeset {} in channel '{}'",
        "✓".green(),
        target_cs.short_change_id().bold(),
        channel_name.cyan()
    );
    println!("  change_id:   {}", revert_cs.change_id);
    println!("  commit_hash: {}", revert_cs.short_commit_hash());
    println!(
        "  {} patch(es), {} total operation(s)",
        revert_cs.patches.len(),
        revert_cs.total_operations()
    );

    Ok(())
}
