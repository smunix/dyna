//! `dyna clone` command implementation.
//!
//! Clones a repository from a remote server, fetching all changesets.

use anyhow::{Context, Result};
use dyna_common::protocol::CloneRequest;
use std::path::PathBuf;

use crate::repository::Repository;
use crate::sync_client::SyncClient;

pub async fn execute(url: String, directory: Option<PathBuf>) -> Result<()> {
    let target_dir = match directory {
        Some(d) => d,
        None => {
            let name = url
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("dyna-repo");
            PathBuf::from(name)
        }
    };

    if target_dir.exists() {
        anyhow::bail!("Directory '{}' already exists", target_dir.display());
    }

    println!("Cloning from {} into {}...", url, target_dir.display());

    std::fs::create_dir_all(&target_dir)?;
    let repo = Repository::init(&target_dir)?;

    // Save the remote URL in config
    let mut config = repo.load_config()?;
    config.remote_url = Some(url.clone());
    repo.save_config(&config)?;

    // Fetch all data from the remote
    let client = SyncClient::new(&url);
    let clone_response = client
        .clone_repo(&CloneRequest { channel: None })
        .await
        .context("Failed to clone from remote")?;

    // Store all changesets locally
    let mut changeset_count = 0;
    for cs in &clone_response.changesets {
        repo.store_changeset(cs)?;
        changeset_count += 1;
    }

    // Store all channels
    for channel in &clone_response.channels {
        repo.save_channel(channel)?;
    }

    // Store resource snapshots
    for (resource_id, snapshot) in &clone_response.snapshots {
        repo.save_snapshot(resource_id, snapshot)?;

        // Also write the resource file to the working directory
        let resource_path = target_dir.join(format!("{}.json", resource_id));
        let json = serde_json::to_string_pretty(snapshot)?;
        std::fs::write(resource_path, json)?;
    }

    // Update sync state
    let mut sync_state = repo.load_sync_state()?;
    for channel in &clone_response.channels {
        if let Some(head) = &channel.head_change_id {
            sync_state
                .remote_heads
                .insert(channel.name.clone(), head.clone());
        }
    }
    repo.save_sync_state(&sync_state)?;

    println!(
        "Clone complete. Fetched {} changeset(s), {} resource(s).",
        changeset_count,
        clone_response.snapshots.len()
    );

    Ok(())
}
