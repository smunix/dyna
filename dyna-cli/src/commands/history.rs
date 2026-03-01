//! `dyna history` — query the change history of a specific resource.
//!
//! Fetches the history of a given resource_id from the remote server,
//! showing all changesets that modified the resource across all channels.
//!
//! Usage:
//!   dyna history acme.entity.User
//!   dyna history acme.entity.User --verbose

use anyhow::{Context, Result};

use crate::repository::Repository;
use crate::sync_client::SyncClient;

/// Execute the `history` command.
///
/// Queries the remote server for the change history of the given resource_id
/// and displays it as a timeline.
pub async fn execute(resource_id: String, verbose: bool) -> Result<()> {
    let repo = Repository::find_current()?;
    let config = repo.load_config()?;

    let remote_url = config
        .remote_url
        .as_deref()
        .context("No remote URL configured. Run `dyna init` or set a remote first.")?;

    let client = SyncClient::new(remote_url);

    println!(
        "Fetching history for \x1b[1m{}\x1b[0m from {}...\n",
        resource_id, remote_url
    );

    let response = client.resource_history(&resource_id).await?;

    if let Some(ref error) = response.error {
        anyhow::bail!("Server error: {}", error);
    }

    if response.entries.is_empty() {
        println!("No history found for resource \x1b[1m{}\x1b[0m.", resource_id);
        println!("The resource may not exist or has no recorded changes.");
        return Ok(());
    }

    println!(
        "History for \x1b[1m{}\x1b[0m ({} entries):\n",
        resource_id,
        response.entries.len()
    );

    for entry in &response.entries {
        let short_hash = &entry.commit_hash[..entry.commit_hash.len().min(8)];
        let short_id = &entry.change_id[..entry.change_id.len().min(8)];

        println!(
            "\x1b[33m●\x1b[0m \x1b[36m{}\x1b[0m  \x1b[2m({})\x1b[0m",
            short_hash, short_id
        );
        println!("  \x1b[1m{}\x1b[0m", entry.message);
        println!(
            "  \x1b[2m{} · {} · channel: {}\x1b[0m",
            entry.author,
            &entry.timestamp[..entry.timestamp.len().min(19)],
            entry.channel
        );

        if verbose && !entry.operations.is_empty() {
            println!("  Operations:");
            for op in &entry.operations {
                if let Some(op_type) = op.get("op").and_then(|v| v.as_str()) {
                    let path = op
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("/");
                    match op_type {
                        "add" => {
                            let value_preview = op
                                .get("value")
                                .map(|v| {
                                    let s = v.to_string();
                                    if s.len() > 60 {
                                        format!("{}…", &s[..60])
                                    } else {
                                        s
                                    }
                                })
                                .unwrap_or_default();
                            println!(
                                "    \x1b[32m+ add\x1b[0m {} = {}",
                                path, value_preview
                            );
                        }
                        "remove" => {
                            println!("    \x1b[31m- remove\x1b[0m {}", path);
                        }
                        "replace" => {
                            let value_preview = op
                                .get("value")
                                .map(|v| {
                                    let s = v.to_string();
                                    if s.len() > 60 {
                                        format!("{}…", &s[..60])
                                    } else {
                                        s
                                    }
                                })
                                .unwrap_or_default();
                            println!(
                                "    \x1b[33m~ replace\x1b[0m {} = {}",
                                path, value_preview
                            );
                        }
                        _ => {
                            println!("    {} {}", op_type, path);
                        }
                    }
                }
            }
        }

        println!();
    }

    Ok(())
}
