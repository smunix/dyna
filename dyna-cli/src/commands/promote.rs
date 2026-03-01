//! `dyna promote` command implementation.
//!
//! Promotes changesets from a source channel to the main channel.
//! By default, promotes from the current channel. Use `--channel` to
//! specify a different source channel.
//!
//! Promotion is **remote-first**: the server must accept the promotion
//! before local state is updated. This ensures the protected `main`
//! channel is always consistent with the remote and that conflicts are
//! detected server-side before the local repo is mutated.
//!
//! Promoted changesets are marked as immutable.

use anyhow::{Result, bail};
use colored::Colorize;
use dyna_core::channel::promote_changesets;
use itertools::izip;

use crate::repository::Repository;
use crate::sync_client::SyncClient;

pub async fn execute(channel: Option<String>) -> Result<()> {
    let repo = Repository::find_current()?;
    let config = repo.load_config()?;
    let current_name = repo.current_channel_name()?;

    // Use the specified channel or fall back to the current channel
    let source_name = channel.unwrap_or_else(|| current_name.clone());

    if source_name == "main" {
        bail!(
            "Cannot promote from 'main' to itself. Specify a feature channel with --channel, \
             or switch to one first."
        );
    }

    println!(
        "Promoting changesets from '{}' to 'main'...",
        source_name.bold().cyan()
    );

    // ── Remote-first: push the promotion to the server ──────────────
    let remote_url = config
        .remote_url
        .as_ref()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No remote URL configured. Promotion requires a remote server \
                 to ensure conflict-free merges. Set 'remote_url' in .dyna/config.toml."
            )
        })?;

    // First, ensure the source channel's changesets are pushed to the remote
    // so the server has the data it needs to promote.
    let source_channel = repo.load_channel(&source_name)?;
    let sync_state = repo.load_sync_state()?;
    let remote_head = sync_state.remote_heads.get(&source_name).cloned();

    let unpushed_ids: Vec<String> = remote_head
        .as_ref()
        .map(|head| {
            izip!(&source_channel.changesets)
                .skip_while(|id| *id != head)
                .skip(1)
                .cloned()
                .collect()
        })
        .unwrap_or_else(|| source_channel.changesets.clone());

    let client = SyncClient::new(remote_url);

    if !unpushed_ids.is_empty() {
        println!(
            "  Pushing {} unpushed changeset(s) to remote first...",
            unpushed_ids.len()
        );
        let changesets: Vec<_> = izip!(&unpushed_ids)
            .map(|id| repo.load_changeset(id))
            .collect::<Result<Vec<_>, _>>()?;

        let push_request = dyna_core::protocol::PushRequest {
            channel: source_name.clone(),
            changesets,
            expected_head: remote_head,
        };
        let push_response = client.push(&push_request).await?;

        // Update sync state after successful push
        if let Some(new_head) = &push_response.new_head {
            let mut sync_state = repo.load_sync_state()?;
            sync_state
                .remote_heads
                .insert(source_name.clone(), new_head.clone());
            repo.save_sync_state(&sync_state)?;
        }

        println!(
            "  {} changeset(s) pushed to remote.",
            push_response.accepted_count
        );
    }

    // Now ask the server to promote
    let promote_request = dyna_core::protocol::PromoteRequest {
        source_channel: source_name.clone(),
        target_channel: "main".to_string(),
    };

    let promote_response = client.promote(&promote_request).await.map_err(|e| {
        anyhow::anyhow!(
            "Remote promotion failed (no local changes made): {}",
            e
        )
    })?;

    if !promote_response.success {
        bail!(
            "Remote promotion rejected (no local changes made): {}",
            promote_response
                .error
                .unwrap_or_else(|| "Unknown conflict".into())
        );
    }

    println!(
        "  Remote promotion succeeded: {} changeset(s) promoted.",
        promote_response.promoted_changesets.len()
    );

    // ── Apply locally only after remote success ─────────────────────
    let source = repo.load_channel(&source_name)?;
    let mut target = repo.load_channel("main")?;

    promote_changesets(&source, &mut target)
        .map_err(|e| anyhow::anyhow!("Local promotion failed: {}", e))
        .and_then(|promoted_ids| {
            // Mark promoted changesets as immutable via try_for_each
            izip!(&promoted_ids)
                .filter_map(|id| repo.load_changeset(id).ok())
                .try_for_each(|mut cs| {
                    cs.immutable = true;
                    repo.store_changeset(&cs)
                })?;

            // Save the updated main channel
            repo.save_channel(&target)?;

            println!(
                "\n{} {} changeset(s) promoted to 'main':",
                "✓".green(),
                promoted_ids.len().to_string().green()
            );

            izip!(&promoted_ids).for_each(|id| {
                repo.load_changeset(id)
                    .map(|cs| {
                        println!(
                            "  {} ({}) -> OK [immutable]",
                            cs.short_change_id(),
                            cs.message
                        );
                    })
                    .unwrap_or_else(|_| {
                        println!(
                            "  {} -> OK [immutable]",
                            &id[..std::cmp::min(id.len(), 8)]
                        );
                    });
            });

            target.head_change_id.as_ref().map(|head| {
                println!(
                    "\nMain HEAD: {}",
                    &head[..std::cmp::min(head.len(), 8)]
                );
            });

            // Update sync state for main channel
            promote_response.new_head.as_ref().map(|new_head| -> Result<()> {
                let mut sync_state = repo.load_sync_state()?;
                sync_state
                    .remote_heads
                    .insert("main".to_string(), new_head.clone());
                repo.save_sync_state(&sync_state)
            }).transpose()?;

            Ok(())
        })
}
