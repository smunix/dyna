//! `dyna clone` command implementation.
//!
//! Clones a repository from a remote server, fetching all changesets.

use anyhow::{Context, Result};
use dyna_core::protocol::CloneRequest;
use itertools::izip;
use std::path::PathBuf;

use crate::repository::Repository;
use crate::sync_client::SyncClient;

pub async fn execute(url: String, directory: Option<PathBuf>) -> Result<()> {
    let target_dir = directory.unwrap_or_else(|| {
        PathBuf::from(
            url.trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("dyna-repo"),
        )
    });

    if target_dir.exists() {
        anyhow::bail!("Directory '{}' already exists", target_dir.display());
    }

    println!("Cloning from {} into {}...", url, target_dir.display());

    std::fs::create_dir_all(&target_dir)?;
    let repo = Repository::init(&target_dir)?;

    // Save the remote URL in config
    repo.load_config()
        .and_then(|mut config| {
            config.remote_url = Some(url.clone());
            repo.save_config(&config)
        })?;

    // Fetch all data from the remote
    let client = SyncClient::new(&url);
    let clone_response = client
        .clone_repo(&CloneRequest { channel: None })
        .await
        .context("Failed to clone from remote")?;

    // Store all changesets locally via try_fold to count
    let changeset_count = izip!(&clone_response.changesets)
        .try_fold(0usize, |count, cs| {
            repo.store_changeset(cs).map(|()| count + 1)
        })?;

    // Store all channels via try_for_each
    izip!(&clone_response.channels)
        .try_for_each(|channel| repo.save_channel(channel))?;

    // Store resource snapshots and write working directory files, recreating
    // the full filesystem hierarchy from dotted resource IDs via path_from_resource_id.
    izip!(&clone_response.snapshots)
        .try_for_each(|(resource_id, snapshot)| -> Result<()> {
            repo.save_snapshot(resource_id, snapshot)?;
            repo.path_from_resource_id(resource_id)
                .and_then(|file_path| {
                    serde_json::to_string_pretty(snapshot)
                        .map_err(Into::into)
                        .and_then(|json| std::fs::write(&file_path, json).map_err(Into::into))
                })
        })?;

    // Update sync state: fold channel heads into sync_state
    izip!(&clone_response.channels)
        .filter_map(|ch| {
            ch.head_change_id
                .as_ref()
                .map(|head| (ch.name.clone(), head.clone()))
        })
        .try_fold(repo.load_sync_state()?, |mut sync_state, (name, head)| {
            sync_state.remote_heads.insert(name, head);
            Ok::<_, anyhow::Error>(sync_state)
        })
        .and_then(|sync_state| repo.save_sync_state(&sync_state))?;

    println!(
        "Clone complete. Fetched {} changeset(s), {} resource(s).",
        changeset_count,
        clone_response.snapshots.len()
    );

    Ok(())
}
