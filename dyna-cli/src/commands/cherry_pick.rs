//! `dyna cherry-pick` command implementation.
//!
//! Cherry-picks a changeset from another channel (or any reachable changeset)
//! and applies its patches as a new changeset on the destination channel.
//!
//! The new changeset copies the patches from the source changeset but records
//! the current channel head as its parent (not the original parent chain).
//! This is a local-only operation that creates a normal commit.

use anyhow::{Result, bail};
use colored::Colorize;
use dyna_core::diff;
use dyna_core::models::{Changeset, Patch, PatchContent};
use itertools::izip;

use crate::repository::Repository;

pub async fn execute(
    change_id: String,
    channel: Option<String>,
) -> Result<()> {
    let repo = Repository::find_current()?;
    let config = repo.load_config()?;

    let dest_channel_name = channel
        .map(Ok)
        .unwrap_or_else(|| repo.current_channel_name())?;

    // Protect the main channel
    if dest_channel_name == "main" {
        bail!(
            "Cannot cherry-pick directly onto the 'main' channel. \
             Switch to a feature channel first, then promote to main."
        );
    }

    // Resolve the source changeset (supports prefix matching)
    let matches = repo.find_changeset_by_prefix(&change_id)?;
    let source_cs = match matches.len() {
        0 => bail!("No changeset found matching '{}'", change_id),
        1 => matches.into_iter().next().unwrap(),
        n => bail!(
            "Ambiguous prefix '{}' matches {} changesets. Please provide a longer prefix.",
            change_id, n
        ),
    };

    println!(
        "Cherry-picking changeset {} ({}) onto '{}'...",
        source_cs.short_change_id().bold(),
        source_cs.message,
        dest_channel_name.cyan()
    );

    if source_cs.patches.is_empty() {
        bail!(
            "Changeset {} has no patches to cherry-pick.",
            source_cs.short_change_id()
        );
    }

    // Check if the changeset is already in the destination channel
    let dest_channel = repo.load_channel(&dest_channel_name)?;
    if dest_channel.changesets.contains(&source_cs.change_id) {
        bail!(
            "Changeset {} is already in channel '{}'.",
            source_cs.short_change_id(),
            dest_channel_name
        );
    }

    // Build new patches by applying the source patches to the destination's
    // current snapshots. For each patch:
    //   1. Load the destination's current snapshot for the resource.
    //   2. Apply the source patch's operations to get the new state.
    //   3. Compute a diff from the destination's snapshot to the new state.
    //   4. Build a new patch with the destination's snapshot as parent.
    let cherry_patches: Vec<Patch> = izip!(&source_cs.patches)
        .map(|src_patch| {
            // Get the current state in the destination channel
            let current_snapshot = repo
                .load_snapshot(&src_patch.target_resource)
                .ok()
                .flatten()
                .unwrap_or_else(|| serde_json::json!({}));

            // The source patch records what it changed: parent_snapshot → result_snapshot.
            // We apply the same logical change to the destination's current state.
            // If the source has result_snapshot, use the diff from source's parent to result
            // applied on top of destination's current state.
            let new_snapshot = src_patch
                .result_snapshot
                .as_ref()
                .and_then(|result| {
                    src_patch.parent_snapshot.as_ref().map(|parent| {
                        // Compute the delta the source changeset introduced
                        let delta_ops = diff::diff(parent, result);
                        // Apply that delta to the destination's current state
                        let mut dest = current_snapshot.clone();
                        diff::apply_patch(&mut dest, &delta_ops).ok();
                        dest
                    })
                })
                .unwrap_or_else(|| {
                    // Fallback: use the source's result_snapshot directly
                    src_patch
                        .result_snapshot
                        .clone()
                        .unwrap_or_else(|| current_snapshot.clone())
                });

            // Compute operations from current → new
            let operations = diff::diff(&current_snapshot, &new_snapshot);

            let content = PatchContent {
                target_resource: src_patch.target_resource.clone(),
                operations,
                parent_snapshot: Some(current_snapshot),
                result_snapshot: Some(new_snapshot),
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

    // Parent is the destination channel's current head
    let parents = dest_channel
        .head_change_id
        .clone()
        .map(|id| vec![id])
        .unwrap_or_default();

    let cherry_message = format!(
        "Cherry-pick \"{}\" ({})",
        source_cs.message,
        source_cs.short_change_id()
    );

    let cherry_cs = Changeset::new(
        config.user.name.clone(),
        cherry_message.clone(),
        parents,
        cherry_patches,
    );

    // Print summary
    izip!(&cherry_cs.patches).for_each(|p| {
        println!(
            "  [{}] {} -> {} op(s)",
            &p.hash[7..std::cmp::min(p.hash.len(), 19)],
            p.target_resource,
            p.operations.len()
        );
    });

    // Store the cherry-pick changeset
    repo.store_changeset(&cherry_cs)?;

    // Update snapshots
    izip!(&cherry_cs.patches).try_for_each(|p| {
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

    // Append to the destination channel
    repo.load_channel(&dest_channel_name)
        .and_then(|mut channel| {
            channel.append_changeset(cherry_cs.change_id.clone());
            repo.save_channel(&channel)
        })?;

    // Set as working change
    repo.set_working_change(Some(&cherry_cs.change_id))?;

    println!(
        "\n{} Cherry-picked changeset {} onto channel '{}'",
        "✓".green(),
        source_cs.short_change_id().bold(),
        dest_channel_name.cyan()
    );
    println!("  new change_id:   {}", cherry_cs.change_id);
    println!("  commit_hash:     {}", cherry_cs.short_commit_hash());
    println!(
        "  {} patch(es), {} total operation(s)",
        cherry_cs.patches.len(),
        cherry_cs.total_operations()
    );

    Ok(())
}
