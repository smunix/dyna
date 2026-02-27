//! `dyna channel` command implementation.
//!
//! Supports:
//! - `dyna channel <name>` — switch to an existing channel
//! - `dyna channel <name> --create` — create and switch to a new channel
//! - `dyna channel --list` — list all local channels
//! - `dyna channel --list --remote` — list local and remote channels
//!
//! Switching channels is blocked if there are staged but uncommitted files.
//! On a successful switch, the working directory is cleaned of tracked JSON
//! files and repopulated with the snapshots from the target channel's patches.

use anyhow::{Result, bail};
use colored::Colorize;
use std::collections::HashMap;

use crate::repository::Repository;
use crate::sync_client::SyncClient;

pub async fn execute(
    name: Option<String>,
    create: bool,
    list: bool,
    remote: bool,
) -> Result<()> {
    let repo = Repository::find_current()?;
    let current = repo.current_channel_name()?;

    // ------------------------------------------------------------------
    // List mode: `dyna channel --list [--remote]`
    // ------------------------------------------------------------------
    if list {
        return list_channels(&repo, &current, remote).await;
    }

    // For non-list operations, a channel name is required.
    let name = match name {
        Some(n) => n,
        None => {
            bail!(
                "A channel name is required unless --list is specified.\n\
                 Usage:\n  \
                   dyna channel <name>            Switch to a channel\n  \
                   dyna channel <name> --create   Create and switch\n  \
                   dyna channel --list            List local channels\n  \
                   dyna channel --list --remote   List local + remote channels"
            );
        }
    };

    // ------------------------------------------------------------------
    // Guard: block switch/create if there are staged uncommitted files
    // ------------------------------------------------------------------
    let staged = repo.load_staged_changes()?;
    if !staged.is_empty() {
        let file_list: Vec<&str> = staged.iter().map(|s| s.file_path.as_str()).collect();
        bail!(
            "Cannot switch channels: you have {} staged but uncommitted file(s):\n  {}\n\n\
             Please commit your staged changes first with 'dyna commit -m \"message\"',\n\
             or unstage them before switching channels.",
            staged.len(),
            file_list.join("\n  ")
        );
    }

    // ------------------------------------------------------------------
    // Create mode: `dyna channel <name> --create`
    // ------------------------------------------------------------------
    if create {
        let channel = repo.create_channel(&name, Some(&current))?;
        repo.set_current_channel(&name)?;

        // Recreate working directory for the new channel (same snapshots as parent)
        recreate_working_directory(&repo, &name)?;

        println!("Created and switched to channel '{}'.", name.bold().cyan());
        println!(
            "  Forked from '{}' with {} patch(es).",
            current,
            channel.patches.len()
        );
        return Ok(());
    }

    // ------------------------------------------------------------------
    // Switch mode: `dyna channel <name>`
    // ------------------------------------------------------------------
    let channels = repo.list_channels()?;
    let channel_names: Vec<&str> = channels.iter().map(|c| c.name.as_str()).collect();

    if !channel_names.contains(&name.as_str()) {
        println!(
            "{} Channel '{}' not found locally.",
            "error:".red().bold(),
            name
        );
        println!("\nAvailable local channels:");
        for ch in &channels {
            let marker = if ch.name == current { " *" } else { "" };
            println!(
                "  {}{}  ({} patches)",
                ch.name.bold(),
                marker.green(),
                ch.patches.len()
            );
        }
        println!(
            "\nUse '{}' to create it.",
            format!("dyna channel {} --create", name).bold()
        );
        return Ok(());
    }

    // Clean working directory and recreate from target channel's state
    cleanup_tracked_json_files(&repo)?;
    recreate_working_directory(&repo, &name)?;

    repo.set_current_channel(&name)?;
    let channel = repo.load_channel(&name)?;

    println!("Switched to channel '{}'.", name.bold().cyan());
    println!(
        "  {} patch(es), HEAD: {}",
        channel.patches.len(),
        channel
            .head
            .as_deref()
            .map(|h| &h[..std::cmp::min(h.len(), 19)])
            .unwrap_or("(none)")
    );

    Ok(())
}

/// Remove all tracked JSON files from the working directory.
///
/// "Tracked" means files that have a corresponding snapshot in `.dyna/snapshots/`.
/// Untracked files are left untouched.
fn cleanup_tracked_json_files(repo: &Repository) -> Result<()> {
    let snapshots = repo.load_all_snapshots()?;
    if snapshots.is_empty() {
        return Ok(());
    }

    // Walk the working directory and remove files whose resource ID matches a snapshot
    let json_files = find_json_files_recursive(&repo.work_dir, &repo.dyna_dir)?;
    let mut removed = 0;

    for file_path in &json_files {
        let resource_id = Repository::resource_id_from_path(file_path.as_ref());
        if snapshots.contains_key(&resource_id) {
            let abs_path = repo.work_dir.join(file_path);
            if abs_path.exists() {
                std::fs::remove_file(&abs_path)?;
                removed += 1;
            }
        }
    }

    if removed > 0 {
        println!(
            "  {} tracked file(s) removed from working directory.",
            removed.to_string().dimmed()
        );
    }

    Ok(())
}

/// Recreate the working directory files from the target channel's committed state.
///
/// This replays all patches in the target channel to compute the final snapshot
/// for each resource, then writes those snapshots as JSON files.
fn recreate_working_directory(repo: &Repository, channel_name: &str) -> Result<()> {
    let channel = repo.load_channel(channel_name)?;

    if channel.patches.is_empty() {
        return Ok(());
    }

    // Build the final state of each resource by replaying patches in order.
    // We use result_snapshot if available, otherwise apply operations incrementally.
    let mut resource_states: HashMap<String, serde_json::Value> = HashMap::new();

    for hash in &channel.patches {
        match repo.load_patch(hash) {
            Ok(patch) => {
                if let Some(result) = &patch.result_snapshot {
                    resource_states.insert(patch.target_resource.clone(), result.clone());
                } else {
                    // Apply operations to the current state
                    let current = resource_states
                        .entry(patch.target_resource.clone())
                        .or_insert_with(|| serde_json::json!({}));
                    let _ = dyna_common::diff::apply_patch(current, &patch.operations);
                }
            }
            Err(_) => {
                // Patch not available locally — skip silently
            }
        }
    }

    // Write each resource's final state as a JSON file in the working directory
    let mut written = 0;
    for (resource_id, value) in &resource_states {
        let file_path = repo.work_dir.join(format!("{}.json", resource_id));
        let json = serde_json::to_string_pretty(value)?;
        std::fs::write(&file_path, json)?;

        // Also update the local snapshot so that `status` sees these files as clean
        repo.save_snapshot(resource_id, value)?;
        written += 1;
    }

    if written > 0 {
        println!(
            "  {} resource file(s) restored from channel '{}'.",
            written.to_string().dimmed(),
            channel_name
        );
    }

    Ok(())
}

/// Recursively find all `.json` files in the working directory, returning
/// paths relative to `work_dir`. Skips `.dyna/` and hidden directories.
fn find_json_files_recursive(
    work_dir: &std::path::Path,
    dyna_dir: &std::path::Path,
) -> Result<Vec<String>> {
    let mut results = Vec::new();
    walk_dir(work_dir, work_dir, dyna_dir, &mut results)?;
    results.sort();
    Ok(results)
}

fn walk_dir(
    base: &std::path::Path,
    dir: &std::path::Path,
    dyna_dir: &std::path::Path,
    results: &mut Vec<String>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    if dir.starts_with(dyna_dir) {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            walk_dir(base, &path, dyna_dir, results)?;
        } else if path.extension().map_or(false, |ext| ext == "json") {
            if let Ok(relative) = path.strip_prefix(base) {
                results.push(relative.display().to_string());
            }
        }
    }
    Ok(())
}

/// List local channels, and optionally remote channels.
async fn list_channels(repo: &Repository, current: &str, include_remote: bool) -> Result<()> {
    // Local channels
    let local_channels = repo.list_channels()?;

    println!("{}:", "Local channels".bold().underline());
    if local_channels.is_empty() {
        println!("  (none)");
    } else {
        for ch in &local_channels {
            let marker = if ch.name == current {
                " * (current)".green().to_string()
            } else {
                String::new()
            };
            let head_display = ch
                .head
                .as_deref()
                .map(|h| &h[..std::cmp::min(h.len(), 12)])
                .unwrap_or("(empty)");
            println!(
                "  {:<20} {:>4} patch(es)  HEAD: {}{}",
                ch.name.bold(),
                ch.patches.len(),
                head_display.dimmed(),
                marker
            );
        }
    }

    // Remote channels
    if include_remote {
        let config = repo.load_config()?;
        match &config.remote_url {
            Some(url) => {
                println!("\n{}:", "Remote channels".bold().underline());
                let client = SyncClient::new(url);
                match client.list_channels().await {
                    Ok(response) => {
                        if response.channels.is_empty() {
                            println!("  (none)");
                        } else {
                            let local_names: std::collections::HashSet<&str> =
                                local_channels.iter().map(|c| c.name.as_str()).collect();

                            for ch in &response.channels {
                                let sync_status = if local_names.contains(ch.name.as_str()) {
                                    if let Ok(local_ch) = repo.load_channel(&ch.name) {
                                        if local_ch.head == ch.head {
                                            " (synced)".green().to_string()
                                        } else {
                                            " (diverged)".yellow().to_string()
                                        }
                                    } else {
                                        String::new()
                                    }
                                } else {
                                    " (remote only)".blue().to_string()
                                };

                                let head_display = ch
                                    .head
                                    .as_deref()
                                    .map(|h| &h[..std::cmp::min(h.len(), 12)])
                                    .unwrap_or("(empty)");
                                println!(
                                    "  {:<20} {:>4} patch(es)  HEAD: {}{}",
                                    ch.name.bold(),
                                    ch.patches.len(),
                                    head_display.dimmed(),
                                    sync_status
                                );
                            }

                            let remote_names: std::collections::HashSet<&str> =
                                response.channels.iter().map(|c| c.name.as_str()).collect();
                            let local_only: Vec<&&str> = local_names
                                .iter()
                                .filter(|n| !remote_names.contains(**n))
                                .collect();

                            if !local_only.is_empty() {
                                println!("\n{}:", "Local only (not on remote)".bold().underline());
                                for name in local_only {
                                    println!("  {}", name.bold());
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!(
                            "  {} Failed to fetch remote channels: {}",
                            "warning:".yellow().bold(),
                            e
                        );
                    }
                }
            }
            None => {
                println!(
                    "\n{} No remote configured. Set 'remote_url' in .dyna/config.toml.",
                    "note:".yellow().bold()
                );
            }
        }
    }

    Ok(())
}
