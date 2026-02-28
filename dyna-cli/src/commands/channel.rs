//! `dyna channel` command implementation.
//!
//! Manages channels (bookmarks into the changeset DAG).
//!
//! - `dyna channel --list` — list all channels (local + remote)
//! - `dyna channel --list --local` — list only local channels
//! - `dyna channel --list --remote` — list only remote channels
//! - `dyna channel <name>` — switch to a channel
//! - `dyna channel <name> --create` — create and switch
//!
//! Switching is blocked if there are staged uncommitted files.
//! On switch, the working directory is cleaned and repopulated from
//! the target channel's committed changeset state.

use anyhow::{Result, bail};
use colored::Colorize;
use dyna_core::diff;
use itertools::{izip, Itertools};
use std::collections::{HashMap, HashSet};

use crate::repository::Repository;
use crate::sync_client::SyncClient;

pub async fn execute(
    name: Option<String>,
    create: bool,
    list: bool,
    remote: bool,
    local: bool,
) -> Result<()> {
    let repo = Repository::find_current()?;

    if list {
        return list_channels(&repo, remote, local).await;
    }

    let name = name.ok_or_else(|| {
        anyhow::anyhow!("Channel name required. Use --list to see available channels.")
    })?;

    // Guard: block switch if staged uncommitted files exist
    let staged = repo.load_staged_changes()?;
    (!staged.is_empty()).then(|| -> Result<()> {
        let file_list = izip!(&staged).map(|s| s.resource_id.as_str()).join("\n  ");
        bail!(
            "Cannot switch channels: you have {} staged but uncommitted file(s):\n  {}\n\n\
             Please commit your staged changes first with 'dyna commit -m \"message\"',\n\
             or unstage them before switching channels.",
            staged.len(),
            file_list
        );
    }).transpose()?;

    let current = repo.current_channel_name()?;

    if create {
        repo.create_channel(&name, Some(&current))?;
        println!("Created channel '{}' (forked from '{}')", name.green(), current);
    }

    let target_channel = repo.load_channel(&name)?;

    if name == current && !create {
        println!("Already on channel '{}'.", name.cyan());
        return Ok(());
    }

    // --- Clean working directory ---
    // Remove tracked files (those with a snapshot) via fold to count removals
    let snapshots = repo.load_all_snapshots()?;
    let removed = izip!(snapshots.keys())
        .map(|resource_id| repo.work_dir.join(format!("{}.json", resource_id)))
        .filter(|file_path| file_path.exists())
        .try_fold(0usize, |count, file_path| {
            std::fs::remove_file(&file_path).map(|()| count + 1).map_err(anyhow::Error::from)
        })?;

    (removed > 0).then(|| {
        println!("  {} tracked file(s) removed from working directory.", removed);
    });

    // Clear all snapshots
    let snapshots_dir = repo.dyna_dir.join("snapshots");
    snapshots_dir.exists().then(|| -> Result<()> {
        std::fs::read_dir(&snapshots_dir)?
            .filter_map(|entry| entry.ok())
            .try_for_each(|entry| std::fs::remove_file(entry.path()).map_err(Into::into))
    }).transpose()?;

    // --- Restore target channel state ---
    // Replay all changesets via fold to compute final resource state
    let resource_state: HashMap<String, serde_json::Value> = izip!(&target_channel.changesets)
        .filter_map(|change_id| repo.load_changeset(change_id).ok())
        .flat_map(|cs| izip!(cs.patches))
        .fold(HashMap::new(), |mut state, patch| {
            let current_val = state
                .entry(patch.target_resource.clone())
                .or_insert_with(|| serde_json::json!({}));

            patch
                .result_snapshot
                .as_ref()
                .map(|result| *current_val = result.clone())
                .unwrap_or_else(|| {
                    let _ = diff::apply_patch(current_val, &patch.operations);
                });

            state
        });

    // Write resource files and snapshots, counting via try_fold
    let restored = izip!(&resource_state)
        .try_fold(0usize, |count, (resource_id, value)| -> Result<usize> {
            let file_path = repo.work_dir.join(format!("{}.json", resource_id));
            serde_json::to_string_pretty(value)
                .map_err(Into::into)
                .and_then(|json| std::fs::write(&file_path, json).map_err(Into::into))
                .and_then(|()| repo.save_snapshot(resource_id, value))
                .map(|()| count + 1)
        })?;

    (restored > 0).then(|| {
        println!("  {} resource file(s) restored from channel '{}'.", restored, name);
    });

    // Switch HEAD
    repo.set_current_channel(&name)?;
    repo.set_working_change(target_channel.head_change_id.as_deref())?;

    let head_display = target_channel
        .head_change_id
        .as_deref()
        .map(|id| &id[..std::cmp::min(id.len(), 8)])
        .unwrap_or("(none)");

    println!("Switched to channel '{}'.", name.green().bold());
    println!(
        "  {} changeset(s), HEAD: {}",
        target_channel.changesets.len(),
        head_display
    );

    Ok(())
}

/// List channels with filtering: `--local` for local only, `--remote` for
/// remote only, or both/neither to show all.
async fn list_channels(repo: &Repository, remote_only: bool, local_only: bool) -> Result<()> {
    let current = repo.current_channel_name()?;
    let local_channels = repo.list_channels()?;

    let show_local = !remote_only;
    let show_remote = !local_only;

    // --- Local channels ---
    show_local.then(|| {
        println!("{}", "Local channels:".bold());
        izip!(&local_channels).for_each(|ch| {
            let marker = (ch.name == current).then(|| "* ").unwrap_or("  ");
            let head_str = ch
                .head_change_id
                .as_deref()
                .map(|id| &id[..std::cmp::min(id.len(), 8)])
                .unwrap_or("(empty)");

            let name_display = (ch.name == current)
                .then(|| ch.name.green().bold().to_string())
                .unwrap_or_else(|| ch.name.clone());

            println!(
                "{}{} ({} changesets, HEAD: {})",
                marker, name_display, ch.changesets.len(), head_str
            );
        });
    });

    // --- Remote channels ---
    if show_remote {
        let config = repo.load_config()?;
        match config.remote_url.as_ref() {
            Some(url) => {
                let client = SyncClient::new(url);
                match client.list_channels().await {
                    Ok(response) => {
                        // Add separator if we also showed local
                        show_local.then(|| println!());

                        println!("{}", "Remote channels:".bold());
                        let local_names: HashSet<String> =
                            izip!(&local_channels).map(|c| c.name.clone()).collect();

                        izip!(&response.channels).for_each(|rch| {
                            let head_str = rch
                                .head_change_id
                                .as_deref()
                                .map(|id| &id[..std::cmp::min(id.len(), 8)])
                                .unwrap_or("(empty)");

                            let sync_status = local_names
                                .contains(&rch.name)
                                .then(|| {
                                    izip!(&local_channels)
                                        .find(|c| c.name == rch.name)
                                        .and_then(|local_ch| {
                                            (local_ch.head_change_id == rch.head_change_id)
                                                .then(|| "synced".green().to_string())
                                        })
                                        .unwrap_or_else(|| "diverged".yellow().to_string())
                                })
                                .unwrap_or_else(|| "remote only".red().to_string());

                            println!(
                                "  {} ({} changesets, HEAD: {}) [{}]",
                                rch.name, rch.changesets.len(), head_str, sync_status
                            );
                        });

                        // Show local-only channels (when showing both)
                        show_local.then(|| {
                            let remote_names: HashSet<String> =
                                izip!(&response.channels).map(|c| c.name.clone()).collect();
                            izip!(&local_channels)
                                .filter(|lch| !remote_names.contains(&lch.name))
                                .for_each(|lch| {
                                    println!(
                                        "  {} ({} changesets) [{}]",
                                        lch.name,
                                        lch.changesets.len(),
                                        "local only".blue()
                                    );
                                });
                        });
                    }
                    Err(e) => {
                        println!(
                            "\n{} Could not fetch remote channels: {}",
                            "warning:".yellow().bold(),
                            e
                        );
                    }
                }
            }
            None => {
                println!("\n{} No remote URL configured.", "note:".blue().bold());
            }
        }
    }

    Ok(())
}
