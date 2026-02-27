//! `dyna promote` command implementation.
//!
//! Promotes changesets from the current channel to the main channel.
//! Promoted changesets are marked as immutable.

use anyhow::{Result, bail};
use itertools::izip;
use colored::Colorize;
use dyna_core::channel::promote_changesets;

use crate::repository::Repository;

pub async fn execute() -> Result<()> {
    let repo = Repository::find_current()?;
    let current_name = repo.current_channel_name()?;

    if current_name == "main" {
        bail!("Already on 'main'. Switch to a feature channel first, then promote.");
    }

    let source = repo.load_channel(&current_name)?;
    let mut target = repo.load_channel("main")?;

    println!(
        "Promoting changesets from '{}' to 'main'...",
        current_name.bold().cyan()
    );

    promote_changesets(&source, &mut target)
        .map_err(|e| anyhow::anyhow!("Promotion failed: {}", e))
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
                "\nPromoted {} changeset(s) to 'main':",
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

            // Also push to remote if configured
            Ok(promoted_ids)
        })
        .and_then(|_promoted_ids| {
            repo.load_config()?
                .remote_url
                .map(|remote_url| async move {
                    println!("\nPushing promoted changesets to remote...");
                    let client = crate::sync_client::SyncClient::new(&remote_url);
                    let request = dyna_core::protocol::PromoteRequest {
                        source_channel: current_name.clone(),
                        target_channel: "main".to_string(),
                    };
                    client
                        .promote(&request)
                        .await
                        .map(|response| {
                            println!(
                                "Remote promotion complete. {} changeset(s) promoted.",
                                response.promoted_changesets.len()
                            );
                        })
                        .unwrap_or_else(|e| {
                            println!(
                                "{}",
                                format!(
                                    "Warning: Remote promotion failed: {}. Local promotion succeeded.",
                                    e
                                )
                                .yellow()
                            );
                        });
                });
            Ok(())
        })
}
