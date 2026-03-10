//! `dyna clone` command implementation.
//!
//! Clones a repository from a remote server, fetching all changesets.
//! All filesystem I/O goes through the Repository's VFS abstraction.

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

    // Build per-channel snapshots by replaying each channel's changesets.
    izip!(&clone_response.channels)
        .try_for_each(|channel| -> Result<()> {
            let mut channel_snapshots: std::collections::HashMap<String, serde_json::Value> =
                std::collections::HashMap::new();
            izip!(&channel.changesets)
                .filter_map(|cid| repo.load_changeset(cid).ok())
                .flat_map(|cs| cs.patches.into_iter())
                .for_each(|p| {
                    let entry = channel_snapshots
                        .entry(p.target_resource.clone())
                        .or_insert_with(|| serde_json::json!({}));
                    p.result_snapshot
                        .as_ref()
                        .map(|result| *entry = result.clone())
                        .unwrap_or_else(|| {
                            let _ = dyna_core::diff::apply_patch(entry, &p.operations);
                        });
                });
            izip!(&channel_snapshots)
                .try_for_each(|(resource_id, value)| {
                    repo.save_snapshot_for_channel(&channel.name, resource_id, value)
                })
        })?;

    // Write working directory files from the current channel's (main) snapshots.
    izip!(&repo.load_all_snapshots()?)
        .try_for_each(|(resource_id, value)| -> Result<()> {
            serde_json::to_string_pretty(value)
                .map_err(Into::into)
                .and_then(|json| repo.write_resource_file(resource_id, &json))
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
        "Clone complete. Fetched {} changeset(s), {} channel(s).",
        changeset_count,
        clone_response.channels.len()
    );

    Ok(())
}
