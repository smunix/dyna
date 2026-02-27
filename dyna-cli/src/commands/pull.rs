//! `dyna pull` command implementation.
//!
//! Fetches remote changesets and merges them into the local state.

use anyhow::Result;
use dyna_core::diff;
use dyna_core::protocol::PullRequest;
use itertools::izip;

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
    let sync_state = repo.load_sync_state()?;
    let local_remote_head = sync_state.remote_heads.get(&channel_name).cloned();

    println!("Pulling from {} (channel: {})...", remote_url, channel_name);

    let client = SyncClient::new(remote_url);
    let request = PullRequest {
        channel: channel_name.clone(),
        since_change_id: local_remote_head.clone(),
    };

    let response = client.pull(&request).await?;

    if response.changesets.is_empty() {
        println!("Already up-to-date.");
        return Ok(());
    }

    println!("Fetched {} new changeset(s).", response.changesets.len());

    let mut channel = repo.load_channel(&channel_name)?;

    // Process each changeset, accumulating whether conflicts were found via fold
    let conflicts_found = izip!(&response.changesets)
        .try_fold(false, |has_conflicts, cs| -> Result<bool> {
            repo.store_changeset(cs)?;

            println!(
                "  {} ({}) — {} patch(es)",
                cs.short_change_id(),
                cs.message,
                cs.patches.len()
            );

            // Apply each patch, tracking conflicts via try_fold
            let changeset_has_conflicts = izip!(&cs.patches)
                .try_fold(false, |patch_conflicts, patch| -> Result<bool> {
                    let resource_id = &patch.target_resource;
                    let current_snapshot = repo.load_snapshot(resource_id)?;

                    let conflict = match (&current_snapshot, &patch.parent_snapshot, &patch.result_snapshot) {
                        (Some(local), Some(base), Some(result)) => {
                            // Three-way merge
                            diff::three_way_merge(base, local, result)
                                .map(|merged| {
                                    repo.save_snapshot(resource_id, &merged)
                                        .map(|()| {
                                            println!("    {} -> merged automatically", resource_id);
                                            false
                                        })
                                })
                                .unwrap_or_else(|mut merge_conflicts| {
                                    izip!(&mut merge_conflicts)
                                        .for_each(|c| c.resource_id = resource_id.clone());
                                    repo.save_conflicts(resource_id, &merge_conflicts)
                                        .map(|()| {
                                            println!(
                                                "    {} -> CONFLICT ({} conflict(s))",
                                                resource_id,
                                                merge_conflicts.len()
                                            );
                                            true
                                        })
                                })?
                        }
                        (None, _, Some(result)) => {
                            repo.save_snapshot(resource_id, result)?;
                            println!("    {} -> new resource", resource_id);
                            false
                        }
                        _ => {
                            current_snapshot
                                .map(|mut current| {
                                    diff::apply_patch(&mut current, &patch.operations)
                                        .map(|()| {
                                            let _ = repo.save_snapshot(resource_id, &current);
                                            println!("    {} -> applied", resource_id);
                                        })
                                        .unwrap_or_else(|e| {
                                            println!("    {} -> FAILED: {}", resource_id, e);
                                        });
                                })
                                .unwrap_or_else(|| {
                                    println!("    {} -> skipped (no local snapshot)", resource_id);
                                });
                            false
                        }
                    };

                    Ok(patch_conflicts || conflict)
                })?;

            // Append changeset to channel if not already present
            (!channel.changesets.contains(&cs.change_id))
                .then(|| channel.append_changeset(cs.change_id.clone()));

            Ok(has_conflicts || changeset_has_conflicts)
        })?;

    // Save updated channel
    repo.save_channel(&channel)?;

    // Update sync state
    response
        .current_head
        .as_ref()
        .map(|head| -> Result<()> {
            let mut sync_state = repo.load_sync_state()?;
            sync_state
                .remote_heads
                .insert(channel_name.clone(), head.clone());
            repo.save_sync_state(&sync_state)
        })
        .transpose()?;

    // Write updated snapshots to working directory via try_for_each
    izip!(&repo.load_all_snapshots()?)
        .try_for_each(|(resource_id, value)| -> Result<()> {
            serde_json::to_string_pretty(value)
                .map_err(Into::into)
                .and_then(|json| {
                    std::fs::write(
                        repo.work_dir.join(format!("{}.json", resource_id)),
                        json,
                    )
                    .map_err(Into::into)
                })
        })?;

    conflicts_found
        .then(|| println!("\nConflicts detected! Use 'dyna resolve <file>' to resolve them."))
        .unwrap_or_else(|| println!("\nPull complete. All changesets merged successfully."));

    Ok(())
}
