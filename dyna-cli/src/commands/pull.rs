//! `dyna pull` command implementation.
//!
//! Fetches remote changesets and merges them into the local state.

use anyhow::Result;
use dyna_core::diff;
use dyna_core::protocol::PullRequest;

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
        since_change_id: local_remote_head.clone(),
    };

    let response = client.pull(&request).await?;

    if response.changesets.is_empty() {
        println!("Already up-to-date.");
        return Ok(());
    }

    println!(
        "Fetched {} new changeset(s).",
        response.changesets.len()
    );

    let mut channel = repo.load_channel(&channel_name)?;
    let mut conflicts_found = false;

    for cs in &response.changesets {
        // Store the changeset locally (also stores its patches)
        repo.store_changeset(cs)?;

        println!(
            "  {} ({}) — {} patch(es)",
            cs.short_change_id(),
            cs.message,
            cs.patches.len()
        );

        // Apply each patch in the changeset
        for patch in &cs.patches {
            let resource_id = &patch.target_resource;
            let current_snapshot = repo.load_snapshot(resource_id)?;

            match (&current_snapshot, &patch.parent_snapshot, &patch.result_snapshot) {
                (Some(local), Some(base), Some(result)) => {
                    // Three-way merge
                    match diff::three_way_merge(base, local, result) {
                        Ok(merged) => {
                            repo.save_snapshot(resource_id, &merged)?;
                            println!(
                                "    {} -> merged automatically",
                                resource_id
                            );
                        }
                        Err(mut merge_conflicts) => {
                            for c in &mut merge_conflicts {
                                c.resource_id = resource_id.clone();
                            }
                            repo.save_conflicts(resource_id, &merge_conflicts)?;
                            conflicts_found = true;
                            println!(
                                "    {} -> CONFLICT ({} conflict(s))",
                                resource_id,
                                merge_conflicts.len()
                            );
                        }
                    }
                }
                (None, _, Some(result)) => {
                    // New resource from remote
                    repo.save_snapshot(resource_id, result)?;
                    println!("    {} -> new resource", resource_id);
                }
                _ => {
                    // Apply operations to current snapshot
                    if let Some(mut current) = current_snapshot {
                        match diff::apply_patch(&mut current, &patch.operations) {
                            Ok(()) => {
                                repo.save_snapshot(resource_id, &current)?;
                                println!("    {} -> applied", resource_id);
                            }
                            Err(e) => {
                                println!("    {} -> FAILED: {}", resource_id, e);
                            }
                        }
                    } else {
                        println!("    {} -> skipped (no local snapshot)", resource_id);
                    }
                }
            }
        }

        // Append changeset to channel if not already present
        if !channel.changesets.contains(&cs.change_id) {
            channel.append_changeset(cs.change_id.clone());
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
        println!("\nPull complete. All changesets merged successfully.");
    }

    Ok(())
}
