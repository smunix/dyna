//! `dyna log` command implementation.

use anyhow::Result;
use colored::Colorize;

use crate::repository::Repository;

pub async fn execute(count: usize) -> Result<()> {
    let repo = Repository::find_current()?;
    let channel_name = repo.current_channel_name()?;
    let channel = repo.load_channel(&channel_name)?;

    if channel.patches.is_empty() {
        println!("No patches on channel '{}'.", channel_name);
        return Ok(());
    }

    println!(
        "Patch history for channel '{}' (showing last {}):\n",
        channel_name.bold().cyan(),
        count
    );

    // Show patches in reverse order (most recent first)
    let patches_to_show: Vec<_> = channel
        .patches
        .iter()
        .rev()
        .take(count)
        .collect();

    for (i, hash) in patches_to_show.iter().enumerate() {
        match repo.load_patch(hash) {
            Ok(patch) => {
                let short_hash = &patch.hash[..std::cmp::min(patch.hash.len(), 19)];
                let is_head = i == 0;

                if is_head {
                    print!("{} ", "HEAD ->".bold().green());
                } else {
                    print!("       ");
                }

                println!("{}", short_hash.yellow());
                println!("       Author:   {}", patch.author);
                println!(
                    "       Date:     {}",
                    patch.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
                );
                println!("       Resource: {}", patch.target_resource);
                println!("       Message:  {}", patch.message);

                if !patch.dependencies.is_empty() {
                    let deps: Vec<String> = patch
                        .dependencies
                        .iter()
                        .map(|d| d[..std::cmp::min(d.len(), 15)].to_string())
                        .collect();
                    println!("       Deps:    [{}]", deps.join(", "));
                }

                println!(
                    "       Ops:      {} operation(s)",
                    patch.operations.len()
                );
                println!();
            }
            Err(_) => {
                println!("  {} (patch data not available locally)", hash.dimmed());
                println!();
            }
        }
    }

    let total = channel.patches.len();
    if total > count {
        println!(
            "... and {} more patch(es). Use '-n {}' to see all.",
            total - count,
            total
        );
    }

    Ok(())
}
