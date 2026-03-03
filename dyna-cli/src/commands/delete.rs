use anyhow::Result;
use itertools::izip;
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
        let main_set: HashSet<&String> = izip!(&main_channel.changesets)
            .map(|id| id)
            .collect();
        let all_promoted = izip!(&target_channel.changesets).all(|id| main_set.contains(id));
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
        let config = repo.load_config()?;
        let remote_url = config
            .remote_url
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No remote URL configured."))?;
        let sync_client = crate::sync_client::SyncClient::new(remote_url);
        let request = dyna_core::protocol::DeleteChannelRequest {
            channel: name.clone(),
            force,
        };
        sync_client
            .delete_channel(&request)
            .await
            .map(|resp| {
                resp.success
                    .then(|| println!("Deleted remote channel '{}'", name))
                    .unwrap_or_else(|| {
                        let err = resp.error.unwrap_or_default();
                        eprintln!("Failed to delete remote channel '{}': {}", name, err);
                    });
            })
            .unwrap_or_else(|e| {
                eprintln!("Failed to delete remote channel '{}': {}", name, e);
            });
    }

    Ok(())
}
