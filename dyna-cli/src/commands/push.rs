//! `dyna push` command implementation.
//!
//! Pushes local changesets to the remote server.
//! Accepts an optional `--channel` argument; defaults to the current channel.

use anyhow::Result;
use dyna_core::protocol::PushRequest;
use itertools::{izip, Itertools};

use crate::repository::Repository;
use crate::sync_client::SyncClient;

pub async fn execute(channel: Option<String>) -> Result<()> {
    let repo = Repository::find_current()?;
    let config = repo.load_config()?;

    let remote_url = config
        .remote_url
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No remote URL configured. Set 'remote_url' in .dyna/config.toml"))?;

    let channel_name = channel
        .map(Ok)
        .unwrap_or_else(|| repo.current_channel_name())?;

    let channel_data = repo.load_channel(&channel_name)?;

    let sync_state = repo.load_sync_state()?;
    let remote_head = sync_state.remote_heads.get(&channel_name).cloned();

    // Find changesets after the remote head using skip_while + skip
    let unpushed_ids: Vec<String> = remote_head
        .as_ref()
        .map(|head| {
            izip!(&channel_data.changesets)
                .skip_while(|id| *id != head)
                .skip(1) // skip the head itself
                .cloned()
                .collect_vec()
        })
        .unwrap_or_else(|| channel_data.changesets.clone());

    if unpushed_ids.is_empty() {
        println!("Everything up-to-date on channel '{}'.", channel_name);
        return Ok(());
    }

    // Load changeset objects via try_collect
    let changesets = izip!(&unpushed_ids)
        .map(|id| repo.load_changeset(id))
        .try_collect::<_, Vec<_>, _>()?;

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
    response
        .new_head
        .as_ref()
        .map(|new_head| -> Result<()> {
            let mut sync_state = repo.load_sync_state()?;
            sync_state
                .remote_heads
                .insert(channel_name.clone(), new_head.clone());
            repo.save_sync_state(&sync_state)
        })
        .transpose()?;

    println!(
        "Push complete. {} changeset(s) accepted.",
        response.accepted_count
    );

    izip!(&changesets).for_each(|cs| {
        println!("  {} ({}) -> OK", cs.short_change_id(), cs.message);
    });

    Ok(())
}
