//! `dyna push` command implementation.

use anyhow::{Result, bail};
use dyna_common::protocol::PushRequest;

use crate::repository::Repository;
use crate::sync_client::SyncClient;

pub async fn execute() -> Result<()> {
    let repo = Repository::find_current()?;
    let config = repo.load_config()?;

    let remote_url = config
        .remote_url
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No remote URL configured. Set 'remote_url' in .dyna/config.toml"))?;

    let channel_name = repo.current_channel_name()?;
    let channel = repo.load_channel(&channel_name)?;

    // Determine which patches haven't been pushed yet
    let sync_state = repo.load_sync_state()?;
    let remote_head = sync_state.remote_heads.get(&channel_name).cloned();

    let unpushed_hashes = channel.patches_since(remote_head.as_deref());

    if unpushed_hashes.is_empty() {
        println!("Everything up-to-date on channel '{}'.", channel_name);
        return Ok(());
    }

    // Load the actual patch objects
    let mut patches = Vec::new();
    for hash in &unpushed_hashes {
        let patch = repo.load_patch(hash)?;
        patches.push(patch);
    }

    println!(
        "Pushing {} patch(es) to {} (channel: {})...",
        patches.len(),
        remote_url,
        channel_name
    );

    let client = SyncClient::new(remote_url);
    let request = PushRequest {
        channel: channel_name.clone(),
        patches: patches.clone(),
        expected_head: remote_head,
    };

    let response = client.push(&request).await?;

    // Update sync state
    let mut sync_state = repo.load_sync_state()?;
    if let Some(new_head) = &response.new_head {
        sync_state
            .remote_heads
            .insert(channel_name.clone(), new_head.clone());
    }
    for patch in &patches {
        if !sync_state.pushed_patches.contains(&patch.hash) {
            sync_state.pushed_patches.push(patch.hash.clone());
        }
    }
    repo.save_sync_state(&sync_state)?;

    println!(
        "Push complete. {} patch(es) accepted.",
        response.accepted_count
    );

    for hash in &unpushed_hashes {
        println!("  {} -> OK", &hash[..std::cmp::min(hash.len(), 19)]);
    }

    Ok(())
}
