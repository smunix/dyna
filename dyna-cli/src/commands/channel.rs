//! `dyna channel` command implementation.

use anyhow::Result;
use colored::Colorize;

use crate::repository::Repository;

pub async fn execute(name: String, create: bool) -> Result<()> {
    let repo = Repository::find_current()?;
    let current = repo.current_channel_name()?;

    if create {
        // Create a new channel, forking from the current one
        let channel = repo.create_channel(&name, Some(&current))?;
        repo.set_current_channel(&name)?;

        println!("Created and switched to channel '{}'.", name.bold().cyan());
        println!(
            "  Forked from '{}' with {} patch(es).",
            current,
            channel.patches.len()
        );
    } else {
        // Switch to an existing channel
        let channels = repo.list_channels()?;
        let channel_names: Vec<&str> = channels.iter().map(|c| c.name.as_str()).collect();

        if !channel_names.contains(&name.as_str()) {
            // List available channels
            println!("Channel '{}' not found. Available channels:", name);
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
                "\nUse 'dyna channel {} --create' to create it.",
                name
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
    }

    Ok(())
}
