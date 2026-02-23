//! `dyna promote` command implementation.

use anyhow::{Result, bail};
use colored::Colorize;
use dyna_common::channel::promote_patches;

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
        "Promoting patches from '{}' to 'main'...",
        current_name.bold().cyan()
    );

    match promote_patches(&source, &mut target) {
        Ok(promoted) => {
            // Save the updated main channel
            repo.save_channel(&target)?;

            println!(
                "\nPromoted {} patch(es) to 'main':",
                promoted.len().to_string().green()
            );
            for hash in &promoted {
                println!(
                    "  {} -> OK",
                    &hash[..std::cmp::min(hash.len(), 19)]
                );
            }

            if let Some(head) = &target.head {
                println!(
                    "\nMain HEAD: {}",
                    &head[..std::cmp::min(head.len(), 19)]
                );
            }

            // Also push to remote if configured
            let config = repo.load_config()?;
            if let Some(remote_url) = &config.remote_url {
                println!("\nPushing promoted patches to remote...");
                let client = crate::sync_client::SyncClient::new(remote_url);
                let request = dyna_common::protocol::PromoteRequest {
                    source_channel: current_name.clone(),
                    target_channel: "main".to_string(),
                };
                match client.promote(&request).await {
                    Ok(response) => {
                        println!(
                            "Remote promotion complete. {} patch(es) promoted.",
                            response.promoted_patches.len()
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
