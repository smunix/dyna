//! `dyna restore` command implementation.
//!
//! Restores a file to its snapshot state. The
//! snapshot source can be:
//!
//! - **Default** (no flags): the current channel's latest snapshot.
//! - `--channel <name>`: the head snapshot from a specific channel, computed
//!   by replaying that channel's changeset history.
//! - `--changeset <id>`: the result snapshot from a specific changeset's patch
//!   targeting the resource, searched across all local and remote channels.
//!
//! All filesystem I/O goes through the Repository's VFS abstraction.

use anyhow::{Result, bail};
use colored::Colorize;
use itertools::{izip, Itertools};
use std::path::PathBuf;

use crate::repository::Repository;

pub async fn execute(
    path: PathBuf,
    channel: Option<String>,
    changeset: Option<String>,
) -> Result<()> {
    let repo = Repository::find_current()?;

    let abs_path = path
        .is_absolute()
        .then(|| path.clone())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join(&path));

    let resource_id = repo.resource_id_from_path(&abs_path);

    // Determine the snapshot value to restore from
    let (snapshot_value, source_description) = match (&channel, &changeset) {
        // Both flags provided — error
        (Some(_), Some(_)) => {
            bail!("Cannot specify both --channel and --changeset. Choose one source.");
        }

        // Restore from a specific changeset
        (None, Some(cs_prefix)) => {
            restore_from_changeset(&repo, &resource_id, cs_prefix)?
        }

        // Restore from a specific channel's head
        (Some(ch_name), None) => {
            restore_from_channel(&repo, &resource_id, ch_name)?
        }

        // Default: restore from the current channel's snapshot
        (None, None) => {
            let snapshot = repo
                .load_snapshot(&resource_id)?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No snapshot found for resource '{}'. The file has never been tracked.",
                        resource_id
                    )
                })?;
            let channel_name = repo.current_channel_name()?;
            (snapshot, format!("current channel '{}'", channel_name))
        }
    };

    // Write the restored file to the filesystem via VFS
    let json = serde_json::to_string_pretty(&snapshot_value)?;
    repo.write_resource_file(&resource_id, &json)?;

    // Update the local snapshot to match the restored content
    repo.save_snapshot(&resource_id, &snapshot_value)?;

    let relative = repo.relative_path_for_resource_id(&resource_id);

    println!(
        "Restored {} from {}",
        relative.green().bold(),
        source_description.cyan()
    );

    Ok(())
}

/// Restore from a specific channel by replaying its changeset history to
/// compute the resource's final state.
///
/// Walks the channel's changesets in order, applying patches that target the
/// given resource via `try_fold`. The last result snapshot found is used.
fn restore_from_channel(
    repo: &Repository,
    resource_id: &str,
    channel_name: &str,
) -> Result<(serde_json::Value, String)> {
    let changesets = repo.load_channel_changesets(channel_name)?;

    // Walk changesets in order; for each one that targets our resource, take
    // the result_snapshot from the matching patch. The final value after all
    // changesets is the channel-head state of the resource.
    let snapshot = izip!(&changesets)
        .flat_map(|cs| izip!(&cs.patches))
        .filter(|patch| patch.target_resource == resource_id)
        .fold(None::<serde_json::Value>, |_acc, patch| {
            patch.result_snapshot.clone()
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Resource '{}' not found in any changeset on channel '{}'",
                resource_id,
                channel_name
            )
        })?;

    Ok((snapshot, format!("channel '{}'", channel_name)))
}

/// Restore from a specific changeset, identified by a change_id prefix.
///
/// Searches all local changesets matching the prefix, then finds the patch
/// targeting the given resource. If the changeset is a deletion (result
/// snapshot is null/None), we report that clearly.
fn restore_from_changeset(
    repo: &Repository,
    resource_id: &str,
    cs_prefix: &str,
) -> Result<(serde_json::Value, String)> {
    let matches = repo.find_changeset_by_prefix(cs_prefix)?;

    let cs = matches
        .len()
        .eq(&0)
        .then(|| -> Result<_> {
            bail!(
                "No changeset found matching prefix '{}'",
                cs_prefix
            )
        })
        .unwrap_or_else(|| {
            (matches.len() > 1)
                .then(|| -> Result<_> {
                    let ids = izip!(&matches)
                        .map(|c| c.short_change_id())
                        .collect_vec()
                        .join(", ");
                    bail!(
                        "Ambiguous prefix '{}' matches {} changesets: {}",
                        cs_prefix,
                        matches.len(),
                        ids
                    )
                })
                .unwrap_or_else(|| Ok(matches.into_iter().next().unwrap()))
        })?;

    // Find the patch targeting our resource within this changeset
    let patch = izip!(&cs.patches)
        .find(|p| p.target_resource == resource_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Changeset {} does not contain a patch for resource '{}'.\n\
                 Resources in this changeset: {}",
                cs.short_change_id(),
                resource_id,
                cs.affected_resources().join(", ")
            )
        })?;

    let snapshot = patch
        .result_snapshot
        .clone()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Changeset {} has a patch for '{}' but no result snapshot (this may be a deletion changeset).\n\
                 Use the parent snapshot instead by restoring from the parent changeset.",
                cs.short_change_id(),
                resource_id
            )
        })?;

    Ok((
        snapshot,
        format!(
            "changeset {} (\"{}\")",
            cs.short_change_id(),
            cs.message
        ),
    ))
}
