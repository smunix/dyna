//! `dyna promote` command implementation.
//!
//! Promotes changesets from the current channel to the main channel.
//! Promoted changesets are marked as immutable.

use anyhow::{Result, bail};
use colored::Colorize;
use dyna_common::channel::promote_changesets;

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

    match promote_changesets(&source, &mut target) {
        Ok(promoted_ids) => {
            // Mark promoted changesets as immutable
            for id in &promoted_ids {
                if let Ok(mut cs) = repo.load_changeset(id) {
                    cs.immutable = true;
                    repo.store_changeset(&cs)?;
                }
            }

            // Save the updated main channel
            repo.save_channel(&target)?;

            println!(
                "\nPromoted {} changeset(s) to 'main':",
                promoted_ids.len().to_string().green()
            );
            for id in &promoted_ids {
                if let Ok(cs) = repo.load_changeset(id) {
                    println!(
                        "  {} ({}) -> OK [immutable]",
                        cs.short_change_id(),
                        cs.message
                    );
                } else {
                    println!(
                        "  {} -> OK [immutable]",
                        &id[..std::cmp::min(id.len(), 8)]
                    );
                }
            }

            if let Some(head) = &target.head_change_id {
                println!(
                    "\nMain HEAD: {}",
                    &head[..std::cmp::min(head.len(), 8)]
                );
            }

            // Also push to remote if configured
            let config = repo.load_config()?;
            if let Some(remote_url) = &config.remote_url {
                println!("\nPushing promoted changesets to remote...");
                let client = crate::sync_client::SyncClient::new(remote_url);
                let request = dyna_common::protocol::PromoteRequest {
                    source_channel: current_name.clone(),
                    target_channel: "main".to_string(),
                };
                match client.promote(&request).await {
                    Ok(response) => {
                        println!(
                            "Remote promotion complete. {} changeset(s) promoted.",
                            response.promoted_changesets.len()
                        );
                    }
                    Err(e) => {
                        println!(
                            "{}",
                            format!(
                                "Warning: Remote promotion failed: {}. Local promotion succeeded.",
                                e
                            )
                            .yellow()
                        );
                    }
                }
            }
        }
        Err(e) => {
            println!("{}", format!("Promotion failed: {}", e).red());
        }
    }

    Ok(())
}
