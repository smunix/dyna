//! `dyna channel` command implementation.
//!
//! Supports:
//! - `dyna channel <name>` — switch to an existing channel
//! - `dyna channel <name> --create` — create and switch to a new channel
//! - `dyna channel --list` — list all local channels
//! - `dyna channel --list --remote` — list local and remote channels

use anyhow::{Result, bail};
use colored::Colorize;

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
    // Create mode: `dyna channel <name> --create`
    // ------------------------------------------------------------------
    if create {
        let channel = repo.create_channel(&name, Some(&current))?;
        repo.set_current_channel(&name)?;

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
                            // Build a set of local channel names for comparison
                            let local_names: std::collections::HashSet<&str> =
                                local_channels.iter().map(|c| c.name.as_str()).collect();

                            for ch in &response.channels {
                                let sync_status = if local_names.contains(ch.name.as_str()) {
                                    // Check if local and remote are in sync
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

                            // Show local-only channels
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
