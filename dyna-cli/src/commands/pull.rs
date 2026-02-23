//! `dyna pull` command implementation.

use anyhow::Result;
use dyna_common::diff;
use dyna_common::protocol::PullRequest;

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

    println!(
        "Pulling from {} (channel: {})...",
        remote_url, channel_name
    );

    let client = SyncClient::new(remote_url);
    let request = PullRequest {
        channel: channel_name.clone(),
        since_hash: local_remote_head.clone(),
    };

    let response = client.pull(&request).await?;

    if response.patches.is_empty() {
        println!("Already up-to-date.");
        return Ok(());
    }

    println!(
        "Fetched {} new patch(es).",
        response.patches.len()
    );

    let mut channel = repo.load_channel(&channel_name)?;
    let mut conflicts_found = false;

    for patch in &response.patches {
        // Store the patch locally
        repo.store_patch(patch)?;

        // Try to apply the patch
        let resource_id = &patch.target_resource;
        let current_snapshot = repo.load_snapshot(resource_id)?;

        match (&current_snapshot, &patch.parent_snapshot, &patch.result_snapshot) {
            (Some(local), Some(base), Some(result)) => {
                // Three-way merge
                match diff::three_way_merge(base, local, result) {
                    Ok(merged) => {
                        repo.save_snapshot(resource_id, &merged)?;
                        println!(
                            "  {} -> merged automatically",
                            &patch.hash[..std::cmp::min(patch.hash.len(), 19)]
                        );
                    }
                    Err(mut merge_conflicts) => {
                        // Set resource_id on conflicts
                        for c in &mut merge_conflicts {
                            c.resource_id = resource_id.clone();
                        }
                        repo.save_conflicts(resource_id, &merge_conflicts)?;
                        conflicts_found = true;
                        println!(
                            "  {} -> CONFLICT ({} conflict(s) in {})",
                            &patch.hash[..std::cmp::min(patch.hash.len(), 19)],
                            merge_conflicts.len(),
                            resource_id
                        );
                    }
                }
            }
            (None, _, Some(result)) => {
                // New resource from remote
                repo.save_snapshot(resource_id, result)?;
                println!(
                    "  {} -> new resource '{}'",
                    &patch.hash[..std::cmp::min(patch.hash.len(), 19)],
                    resource_id
                );
            }
            _ => {
                // Apply operations to current snapshot
                if let Some(mut current) = current_snapshot {
                    match diff::apply_patch(&mut current, &patch.operations) {
                        Ok(()) => {
                            repo.save_snapshot(resource_id, &current)?;
                            println!(
                                "  {} -> applied",
                                &patch.hash[..std::cmp::min(patch.hash.len(), 19)]
                            );
                        }
                        Err(e) => {
                            println!(
                                "  {} -> FAILED to apply: {}",
                                &patch.hash[..std::cmp::min(patch.hash.len(), 19)],
                                e
                            );
                        }
                    }
                } else {
                    println!(
                        "  {} -> skipped (no local snapshot for '{}')",
                        &patch.hash[..std::cmp::min(patch.hash.len(), 19)],
                        resource_id
                    );
                }
            }
        }

        // Append to channel if not already present
        if !channel.patches.contains(&patch.hash) {
            channel.append_patch(patch.hash.clone());
        }
    }

    // Save updated channel
    repo.save_channel(&channel)?;

    // Update sync state
    let mut sync_state = repo.load_sync_state()?;
    if let Some(head) = &response.current_head {
        sync_state
            .remote_heads
            .insert(channel_name.clone(), head.clone());
    }
    repo.save_sync_state(&sync_state)?;

    // Write updated snapshots to working directory
    let snapshots = repo.load_all_snapshots()?;
    for (resource_id, value) in &snapshots {
        let resource_path = repo.work_dir.join(format!("{}.json", resource_id));
        let json = serde_json::to_string_pretty(value)?;
        std::fs::write(resource_path, json)?;
    }

    if conflicts_found {
        println!("\nConflicts detected! Use 'dyna resolve <file>' to resolve them.");
    } else {
        println!("\nPull complete. All patches merged successfully.");
    }

    Ok(())
}
