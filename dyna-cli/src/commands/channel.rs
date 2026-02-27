//! `dyna channel` command implementation.
//!
//! Manages channels (bookmarks into the changeset DAG).
//!
//! - `dyna channel --list` — list local channels with changeset counts
//! - `dyna channel --list --remote` — also show remote channels
//! - `dyna channel <name>` — switch to a channel
//! - `dyna channel <name> --create` — create and switch
//!
//! Switching is blocked if there are staged uncommitted files.
//! On switch, the working directory is cleaned and repopulated from
//! the target channel's committed changeset state.

use anyhow::{Result, bail};
use colored::Colorize;
use dyna_core::diff;

use crate::repository::Repository;
use crate::sync_client::SyncClient;

pub async fn execute(
    name: Option<String>,
    create: bool,
    list: bool,
    remote: bool,
) -> Result<()> {
    let repo = Repository::find_current()?;

    if list {
        return list_channels(&repo, remote).await;
    }

    let name = match name {
        Some(n) => n,
        None => bail!("Channel name required. Use --list to see available channels."),
    };

    // Guard: block switch if staged uncommitted files exist
    let staged = repo.load_staged_changes()?;
    if !staged.is_empty() {
        let file_list: Vec<String> = staged.iter().map(|s| s.resource_id.clone()).collect();
        bail!(
            "Cannot switch channels: you have {} staged but uncommitted file(s):\n  {}\n\n\
             Please commit your staged changes first with 'dyna commit -m \"message\"',\n\
             or unstage them before switching channels.",
            staged.len(),
            file_list.join("\n  ")
        );
    }

    let current = repo.current_channel_name()?;

    if create {
        // Create channel, forking from the current channel
        repo.create_channel(&name, Some(&current))?;
        println!("Created channel '{}' (forked from '{}')", name.green(), current);
    }

    // Verify the target channel exists
    let target_channel = repo.load_channel(&name)?;

    if name == current && !create {
        println!("Already on channel '{}'.", name.cyan());
        return Ok(());
    }

    // --- Clean working directory ---
    // Remove tracked files (those with a snapshot)
    let snapshots = repo.load_all_snapshots()?;
    let mut removed = 0;
    for resource_id in snapshots.keys() {
        let file_path = repo.work_dir.join(format!("{}.json", resource_id));
        if file_path.exists() {
            std::fs::remove_file(&file_path)?;
            removed += 1;
        }
    }
    if removed > 0 {
        println!(
            "  {} tracked file(s) removed from working directory.",
            removed
        );
    }

    // Clear all snapshots
    let snapshots_dir = repo.dyna_dir.join("snapshots");
    if snapshots_dir.exists() {
        for entry in std::fs::read_dir(&snapshots_dir)? {
            let entry = entry?;
            std::fs::remove_file(entry.path())?;
        }
    }

    // --- Restore target channel state ---
    // Replay all changesets in the target channel to compute final resource state
    let mut resource_state: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();

    for change_id in &target_channel.changesets {
        if let Ok(cs) = repo.load_changeset(change_id) {
            for patch in &cs.patches {
                let current_val = resource_state
                    .entry(patch.target_resource.clone())
                    .or_insert_with(|| serde_json::json!({}));

                if let Some(ref result) = patch.result_snapshot {
                    *current_val = result.clone();
                } else {
                    let _ = diff::apply_patch(current_val, &patch.operations);
                }
            }
        }
    }

    // Write resource files and snapshots
    let mut restored = 0;
    for (resource_id, value) in &resource_state {
        let file_path = repo.work_dir.join(format!("{}.json", resource_id));
        let json = serde_json::to_string_pretty(value)?;
        std::fs::write(&file_path, json)?;
        repo.save_snapshot(resource_id, value)?;
        restored += 1;
    }
    if restored > 0 {
        println!(
            "  {} resource file(s) restored from channel '{}'.",
            restored,
            name
        );
    }

    // Switch HEAD
    repo.set_current_channel(&name)?;

    // Update working change to the target channel's head
    repo.set_working_change(target_channel.head_change_id.as_deref())?;

    println!("Switched to channel '{}'.", name.green().bold());
    println!(
        "  {} changeset(s), HEAD: {}",
        target_channel.changesets.len(),
        target_channel
            .head_change_id
            .as_deref()
            .map(|id| &id[..std::cmp::min(id.len(), 8)])
            .unwrap_or("(none)")
    );

    Ok(())
}

/// List local channels (and optionally remote channels).
async fn list_channels(repo: &Repository, include_remote: bool) -> Result<()> {
    let current = repo.current_channel_name()?;
    let local_channels = repo.list_channels()?;

    println!("{}", "Local channels:".bold());
    for ch in &local_channels {
        let marker = if ch.name == current { "* " } else { "  " };
        let head_str = ch
            .head_change_id
            .as_deref()
            .map(|id| &id[..std::cmp::min(id.len(), 8)])
            .unwrap_or("(empty)");

        if ch.name == current {
            println!(
                "{}{} ({} changesets, HEAD: {})",
                marker,
                ch.name.green().bold(),
                ch.changesets.len(),
                head_str
            );
        } else {
            println!(
                "{}{} ({} changesets, HEAD: {})",
                marker,
                ch.name,
                ch.changesets.len(),
                head_str
            );
        }
    }

    if include_remote {
        let config = repo.load_config()?;
        if let Some(ref url) = config.remote_url {
            let client = SyncClient::new(url);
            match client.list_channels().await {
                Ok(response) => {
                    println!("\n{}", "Remote channels:".bold());
                    let local_names: std::collections::HashSet<String> =
                        local_channels.iter().map(|c| c.name.clone()).collect();

                    for rch in &response.channels {
                        let head_str = rch
                            .head_change_id
                            .as_deref()
                            .map(|id| &id[..std::cmp::min(id.len(), 8)])
                            .unwrap_or("(empty)");

                        let sync_status = if local_names.contains(&rch.name) {
                            let local = local_channels
                                .iter()
                                .find(|c| c.name == rch.name)
                                .unwrap();
                            if local.head_change_id == rch.head_change_id {
                                "synced".green().to_string()
                            } else {
                                "diverged".yellow().to_string()
                            }
                        } else {
                            "remote only".red().to_string()
                        };

                        println!(
                            "  {} ({} changesets, HEAD: {}) [{}]",
                            rch.name,
                            rch.changesets.len(),
                            head_str,
                            sync_status
                        );
                    }

                    // Show local-only channels
                    let remote_names: std::collections::HashSet<String> =
                        response.channels.iter().map(|c| c.name.clone()).collect();
                    for lch in &local_channels {
                        if !remote_names.contains(&lch.name) {
                            println!(
                                "  {} ({} changesets) [{}]",
                                lch.name,
                                lch.changesets.len(),
                                "local only".blue()
                            );
                        }
                    }
                }
                Err(e) => {
                    println!(
                        "\n{} Could not fetch remote channels: {}",
                        "warning:".yellow().bold(),
                        e
                    );
                }
            }
        } else {
            println!(
                "\n{} No remote URL configured.",
                "note:".blue().bold()
            );
        }
    }

    Ok(())
}
