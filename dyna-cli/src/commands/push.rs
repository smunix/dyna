//! `dyna push` command implementation.
//!
//! Pushes local changesets to the remote server.

use anyhow::Result;
use dyna_core::protocol::PushRequest;

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

    // Determine which changesets haven't been pushed yet
    let sync_state = repo.load_sync_state()?;
    let remote_head = sync_state.remote_heads.get(&channel_name).cloned();

    // Find changesets after the remote head
    let unpushed_ids: Vec<String> = if let Some(ref head) = remote_head {
        let mut found = false;
        channel
            .changesets
            .iter()
            .filter(|id| {
                if found {
                    return true;
                }
                if *id == head {
                    found = true;
                }
                false
            })
            .cloned()
            .collect()
    } else {
        channel.changesets.clone()
    };

    if unpushed_ids.is_empty() {
        println!("Everything up-to-date on channel '{}'.", channel_name);
        return Ok(());
    }

    // Load the changeset objects
    let mut changesets = Vec::new();
    for id in &unpushed_ids {
        let cs = repo.load_changeset(id)?;
        changesets.push(cs);
    }

    println!(
        "Pushing {} changeset(s) to {} (channel: {})...",
        changesets.len(),
        remote_url,
        channel_name
    );

    let client = SyncClient::new(remote_url);
    let request = PushRequest {
        channel: channel_name.clone(),
        changesets: changesets.clone(),
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
    repo.save_sync_state(&sync_state)?;

    println!(
        "Push complete. {} changeset(s) accepted.",
        response.accepted_count
    );

    for cs in &changesets {
        println!(
            "  {} ({}) -> OK",
            cs.short_change_id(),
            cs.message
        );
    }

    Ok(())
}
