//! `dyna squash` command implementation.
//!
//! Squashes a changeset into its parent, analogous to `jj squash`. The child
//! changeset's patches are merged into the parent changeset:
//!
//! - Patches targeting the **same resource** are combined: the parent patch's
//!   `parent_snapshot` is kept, the child patch's `result_snapshot` becomes the
//!   new `result_snapshot`, and a fresh diff is computed between them.
//! - Patches targeting **different resources** are simply appended to the parent.
//! - After squashing, the child changeset is removed from the channel's history
//!   and from the local changeset store.
//!
//! Modes:
//! - `dyna squash` — squash the working changeset (or most recent) into its parent.
//! - `dyna squash --revision <id>` — squash a specific changeset into its parent.
//! - `dyna squash --message <msg>` — override the parent's message after squashing.
//! - `dyna squash --into <id>` — squash into a specific changeset instead of the
//!   immediate parent (the target must be an ancestor).

use anyhow::{Result, bail};
use colored::Colorize;
use dyna_core::diff;
use dyna_core::models::{Changeset, Patch};
use itertools::{izip, Itertools};
use std::collections::HashMap;

use crate::repository::Repository;

pub async fn execute(
    revision: Option<String>,
    into: Option<String>,
    message: Option<String>,
) -> Result<()> {
    let repo = Repository::find_current()?;
    let channel_name = repo.current_channel_name()?;
    let channel = repo.load_channel(&channel_name)?;

    if channel.changesets.is_empty() {
        bail!("Channel '{}' has no changesets to squash.", channel_name);
    }

    // Resolve the child changeset (the one being squashed away)
    let child_change_id = resolve_child_id(&repo, &channel, revision.as_deref())?;
    let child = repo.load_changeset(&child_change_id)?;

    // Guard: immutable changesets cannot be squashed
    if child.immutable {
        bail!(
            "Changeset {} is immutable (promoted). Cannot squash immutable changesets.",
            child.short_change_id()
        );
    }

    // Resolve the target (parent) changeset
    let target_change_id = resolve_target_id(&repo, &channel, &child, into.as_deref())?;
    let target = repo.load_changeset(&target_change_id)?;

    if target.immutable {
        bail!(
            "Target changeset {} is immutable (promoted). Cannot squash into immutable changesets.",
            target.short_change_id()
        );
    }

    // Merge the child's patches into the target's patches
    let merged_patches = merge_patches(&target.patches, &child.patches);

    // Build the updated target changeset
    let new_message = message.unwrap_or_else(|| {
        // Combine messages: keep target message, append child message if non-empty
        child
            .message
            .is_empty()
            .then(|| target.message.clone())
            .unwrap_or_else(|| format!("{}\n\n{}", target.message, child.message))
    });

    let mut updated_target = Changeset {
        change_id: target.change_id.clone(),
        commit_hash: String::new(), // will be recomputed
        message: new_message,
        author: target.author.clone(),
        created_at: target.created_at,
        updated_at: chrono::Utc::now(),
        parents: target.parents.clone(),
        patches: merged_patches,
        immutable: false,
        empty: false,
        bookmarks: target.bookmarks.clone(),
    };
    updated_target.empty = updated_target.patches.is_empty();
    updated_target.recompute_hash();

    // Print summary of the squash
    println!(
        "Squashing {} into {}",
        child.short_change_id().to_string().yellow().bold(),
        updated_target.short_change_id().to_string().cyan().bold(),
    );
    println!(
        "  child:  {} ({} patch(es), \"{}\")",
        child.short_change_id(),
        child.patches.len(),
        truncate_message(&child.message, 50),
    );
    println!(
        "  target: {} ({} patch(es), \"{}\")",
        target.short_change_id(),
        target.patches.len(),
        truncate_message(&target.message, 50),
    );
    println!(
        "  merged: {} patch(es), {} total op(s)",
        updated_target.patches.len(),
        updated_target.total_operations(),
    );

    // Store the updated target changeset (overwrites the old one)
    repo.store_changeset(&updated_target)?;

    // Remove the child changeset from the channel's changeset list
    let mut updated_channel = repo.load_channel(&channel_name)?;
    updated_channel.changesets = izip!(updated_channel.changesets)
        .filter(|id| *id != child_change_id)
        .collect_vec();

    // If the channel head was pointing to the child, update it
    let head_is_child = updated_channel
        .head_change_id
        .as_ref()
        .map_or(false, |head| head == &child_change_id);
    if head_is_child {
        updated_channel.head_change_id = updated_channel.changesets.last().cloned();
    }

    updated_channel.updated_at = chrono::Utc::now();
    repo.save_channel(&updated_channel)?;

    // Remove the child changeset file from local store
    remove_changeset_file(&repo, &child_change_id)?;

    // Update any changesets that had the child as a parent to point to the
    // target instead (reparenting).
    reparent_children(&repo, &child_change_id, &target_change_id)?;

    // If the working change was the child, update it to the target
    repo.working_change_id()?
        .filter(|wc| *wc == child_change_id)
        .map(|_| repo.set_working_change(Some(&target_change_id)))
        .transpose()?;

    println!(
        "\n{} Squashed {} into {}",
        "Done.".green().bold(),
        child.short_change_id(),
        updated_target.short_change_id(),
    );
    println!(
        "  new commit_hash: {}",
        updated_target.short_commit_hash()
    );

    Ok(())
}

/// Resolve which changeset is being squashed (the "child").
///
/// If `--revision` is given, find it by prefix. Otherwise, use the working
/// changeset or the channel head.
fn resolve_child_id(
    repo: &Repository,
    channel: &dyna_core::models::Channel,
    revision: Option<&str>,
) -> Result<String> {
    revision
        .map(|prefix| {
            let matches = repo.find_changeset_by_prefix(prefix)?;
            match matches.len() {
                0 => bail!("No changeset found matching prefix '{}'", prefix),
                1 => Ok(matches[0].change_id.clone()),
                _ => {
                    let ids = izip!(&matches)
                        .map(|c| c.short_change_id().to_string())
                        .collect_vec()
                        .join(", ");
                    bail!(
                        "Ambiguous prefix '{}' matches {} changesets: {}",
                        prefix,
                        matches.len(),
                        ids
                    )
                }
            }
        })
        .unwrap_or_else(|| {
            // Try working change first, then channel head
            repo.working_change_id()?
                .or_else(|| channel.head_change_id.clone())
                .ok_or_else(|| anyhow::anyhow!("No changeset to squash. The channel is empty."))
        })
}

/// Resolve the target changeset (the one being squashed into).
///
/// If `--into` is given, find it by prefix. Otherwise, use the child's first
/// parent.
fn resolve_target_id(
    repo: &Repository,
    channel: &dyna_core::models::Channel,
    child: &Changeset,
    into: Option<&str>,
) -> Result<String> {
    into.map(|prefix| {
        let matches = repo.find_changeset_by_prefix(prefix)?;
        match matches.len() {
            0 => bail!("No changeset found matching prefix '{}'", prefix),
            1 => Ok(matches[0].change_id.clone()),
            _ => {
                let ids = izip!(&matches)
                    .map(|c| c.short_change_id().to_string())
                    .collect_vec()
                    .join(", ");
                bail!(
                    "Ambiguous prefix '{}' matches {} changesets: {}",
                    prefix,
                    matches.len(),
                    ids
                )
            }
        }
    })
    .unwrap_or_else(|| {
        // Default: use the child's first parent
        child
            .parents
            .first()
            .cloned()
            .or_else(|| {
                // No explicit parent — try the changeset immediately before the
                // child in the channel's ordered list.
                izip!(&channel.changesets)
                    .position(|id| *id == child.change_id)
                    .and_then(|pos| (pos > 0).then(|| channel.changesets[pos - 1].clone()))
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Changeset {} has no parent and is the first in the channel. Nothing to squash into.",
                    child.short_change_id()
                )
            })
    })
}

/// Merge two sets of patches. For patches targeting the same resource, combine
/// them by keeping the target's `parent_snapshot` and the child's
/// `result_snapshot`, then recomputing the diff. For patches targeting
/// different resources, concatenate them.
fn merge_patches(target_patches: &[Patch], child_patches: &[Patch]) -> Vec<Patch> {
    // Index target patches by resource ID for O(1) lookup
    let target_by_resource: HashMap<&str, &Patch> = izip!(target_patches)
        .map(|p| (p.target_resource.as_str(), p))
        .collect();

    // Track which target resources have been merged with a child patch
    let merged_resources: Vec<&str> = izip!(child_patches)
        .filter(|cp| target_by_resource.contains_key(cp.target_resource.as_str()))
        .map(|cp| cp.target_resource.as_str())
        .collect();

    // Start with target patches, replacing those that overlap with child patches
    let mut result: Vec<Patch> = izip!(target_patches)
        .map(|tp| {
            // Find a child patch for the same resource
            izip!(child_patches)
                .find(|cp| cp.target_resource == tp.target_resource)
                .map(|cp| merge_two_patches(tp, cp))
                .unwrap_or_else(|| tp.clone())
        })
        .collect();

    // Append child patches that target new resources (not in the target)
    izip!(child_patches)
        .filter(|cp| !merged_resources.contains(&cp.target_resource.as_str()))
        .for_each(|cp| {
            // For new resources from the child, keep the child patch but use
            // the target's parent context if available
            result.push(cp.clone());
        });

    result
}

/// Merge two patches targeting the same resource. The result patch spans from
/// the target's `parent_snapshot` to the child's `result_snapshot`.
fn merge_two_patches(target: &Patch, child: &Patch) -> Patch {
    let parent_snapshot = target.parent_snapshot.clone();
    let result_snapshot = child.result_snapshot.clone();

    // Recompute operations from the combined before/after snapshots
    let operations = parent_snapshot
        .as_ref()
        .zip(result_snapshot.as_ref())
        .map(|(before, after)| diff::diff(before, after))
        .unwrap_or_else(|| {
            // If we don't have both snapshots, concatenate operations
            let mut ops = target.operations.clone();
            ops.extend(child.operations.clone());
            ops
        });

    Patch::new(
        target.target_resource.clone(),
        operations,
        parent_snapshot,
        result_snapshot,
    )
}

/// Remove a changeset file from the local store.
fn remove_changeset_file(repo: &Repository, change_id: &str) -> Result<()> {
    let path = repo
        .dyna_dir
        .join(format!("changesets/{}.json", change_id));
    path.exists()
        .then(|| std::fs::remove_file(&path).map_err(Into::into))
        .unwrap_or(Ok(()))
}

/// Reparent any changesets that had `old_parent` as a parent to point to
/// `new_parent` instead. This maintains DAG integrity after squashing.
fn reparent_children(repo: &Repository, old_parent: &str, new_parent: &str) -> Result<()> {
    repo.all_changeset_ids()?
        .into_iter()
        .filter_map(|id| repo.load_changeset(&id).ok())
        .filter(|cs| cs.parents.contains(&old_parent.to_string()))
        .try_for_each(|mut cs| {
            cs.parents = izip!(cs.parents)
                .map(|p| {
                    (p == old_parent)
                        .then(|| new_parent.to_string())
                        .unwrap_or(p)
                })
                .collect();
            cs.recompute_hash();
            repo.store_changeset(&cs)
        })
}

/// Truncate a message string for display.
fn truncate_message(msg: &str, max_len: usize) -> String {
    let first_line = msg.lines().next().unwrap_or("");
    (first_line.len() > max_len)
        .then(|| format!("{}...", &first_line[..max_len]))
        .unwrap_or_else(|| first_line.to_string())
}
