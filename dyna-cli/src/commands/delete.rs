use anyhow::Result;
use std::collections::HashSet;

/// Execute the `delete` command.
///
/// Deletes a channel locally, remotely, or both. Refuses to delete "main".
/// Unless `--force` is set, verifies all channel changesets are in main.
pub async fn execute(
    name: String,
    force: bool,
    local: bool,
    remote: bool,
    both: bool,
) -> Result<()> {
    // Protect main
    if name == "main" {
        anyhow::bail!("Cannot delete the 'main' channel: it is protected.");
    }

    // Determine targets: default to local-only
    let (do_local, do_remote) = if both {
        (true, true)
    } else if remote {
        (false, true)
    } else {
        // default or --local
        (true, false)
    };

    let repo = crate::repository::Repository::find_current()?;

    // Check promotion status unless --force
    if !force {
        let main_channel = repo.load_channel("main")?;
        let target_channel = repo.load_channel(&name)?;
        let main_set: HashSet<&String> = main_channel.changesets.iter().collect();
        let all_promoted = target_channel
            .changesets
            .iter()
            .all(|id| main_set.contains(id));
        if !all_promoted {
            anyhow::bail!(
                "Channel '{}' has not been fully promoted to main. Use --force to delete anyway.",
                name
            );
        }
    }

    if do_local {
        repo.delete_channel(&name)?;
        println!("Deleted local channel '{}'", name);
    }

    if do_remote {
        let sync_client = crate::sync_client::SyncClient::from_repo(&repo)?;
        let request = dyna_core::protocol::DeleteChannelRequest {
            channel: name.clone(),
            force,
        };
        match sync_client.delete_channel(&request).await {
            Ok(resp) => {
                if resp.success {
                    println!("Deleted remote channel '{}'", name);
                } else {
                    let err = resp.error.unwrap_or_default();
                    eprintln!("Failed to delete remote channel '{}': {}", name, err);
                }
            }
            Err(e) => {
                eprintln!("Failed to delete remote channel '{}': {}", name, e);
            }
        }
    }

    Ok(())
}
