//! `dyna push` command implementation.
//!
//! Pushes local changesets to the remote server.
//! Accepts an optional `--channel` argument; defaults to the current channel.
//!
//! **Smart dedup**: only sends changesets that the remote server does not
//! already have on the target channel.  The sync state tracks the last-known
//! remote head per channel, so we only send changesets after that head.
//!
//! **Force mode** (`--force`): resends ALL local changesets for the channel,
//! ignoring the sync state entirely. This is useful when the remote server
//! has lost its in-memory state (e.g. after a restart with in-memory VFS).
//! The server will re-store each changeset and rebuild the channel.

use anyhow::{Result, bail};
use dyna_core::protocol::PushRequest;
use itertools::{izip, Itertools};

use crate::repository::Repository;
use crate::sync_client::SyncClient;

pub async fn execute(channel: Option<String>, force: bool) -> Result<()> {
    let repo = Repository::find_current()?;
    let config = repo.load_config()?;

    let remote_url = config
        .remote_url
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No remote URL configured. Set 'remote_url' in .dyna/config.toml"))?;

    let channel_name = channel
        .map(Ok)
        .unwrap_or_else(|| repo.current_channel_name())?;

    // Protect the main channel from direct pushes
    if channel_name == "main" {
        bail!(
            "Cannot push directly to the 'main' channel. \
             Push to a feature channel and use 'dyna promote' instead."
        );
    }

    let channel_data = repo.load_channel(&channel_name)?;

    // Determine which changesets to push
    let (unpushed_ids, expected_head) = if force {
        // Force mode: send ALL changesets, ignore sync state
        println!(
            "\x1b[33mForce push:\x1b[0m resending all {} changeset(s) for channel '{}'...",
            channel_data.changesets.len(),
            channel_name
        );
        (channel_data.changesets.clone(), None)
    } else {
        // Normal mode: only send changesets after the remote head
        let sync_state = repo.load_sync_state()?;
        let remote_head = sync_state.remote_heads.get(&channel_name).cloned();

        let ids: Vec<String> = remote_head
            .as_ref()
            .map(|head| {
                izip!(&channel_data.changesets)
                    .skip_while(|id| *id != head)
                    .skip(1) // skip the head itself
                    .cloned()
                    .collect_vec()
            })
            .unwrap_or_else(|| channel_data.changesets.clone());

        (ids, remote_head)
    };

    if unpushed_ids.is_empty() {
        println!("Everything up-to-date on channel '{}'.", channel_name);
        return Ok(());
    }

    // Load changeset objects
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
        expected_head,
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

    if force {
        println!(
            "\x1b[32mForce push complete.\x1b[0m {} changeset(s) synced.",
            response.accepted_count
        );
    } else {
        println!(
            "Push complete. {} changeset(s) accepted.",
            response.accepted_count
        );
    }

    izip!(&changesets).for_each(|cs| {
        println!("  {} ({}) -> OK", cs.short_change_id(), cs.message);
    });

    Ok(())
}
