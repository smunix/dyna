//! `dyna status` command implementation.

use anyhow::Result;
use colored::Colorize;

use crate::repository::Repository;

pub async fn execute() -> Result<()> {
    let repo = Repository::find_current()?;
    let config = repo.load_config()?;
    let channel_name = repo.current_channel_name()?;
    let channel = repo.load_channel(&channel_name)?;

    // Header
    println!("On channel {}", channel_name.bold().cyan());
    if let Some(head) = &channel.head {
        println!("  HEAD: {}", &head[..std::cmp::min(head.len(), 19)]);
    } else {
        println!("  HEAD: (no commits yet)");
    }
    println!(
        "  Total patches: {}",
        channel.patches.len()
    );

    // Remote info
    if let Some(url) = &config.remote_url {
        let sync_state = repo.load_sync_state()?;
        let remote_head = sync_state.remote_heads.get(&channel_name);
        let unpushed = channel.patches_since(remote_head.map(|s| s.as_str()));
        if unpushed.is_empty() {
            println!("  Remote: {} (up-to-date)", url);
        } else {
            println!(
                "  Remote: {} ({} unpushed patch(es))",
                url,
                unpushed.len().to_string().yellow()
            );
        }
    } else {
        println!("  Remote: (not configured)");
    }

    // Staged changes
    let staged = repo.load_staged_changes()?;
    if staged.is_empty() {
        println!("\n{}", "No staged changes.".dimmed());
    } else {
        println!("\n{}:", "Staged changes".green().bold());
        for change in &staged {
            let op_count = change.operations.len();
            let status = if change.previous.is_none() {
                "new".green()
            } else {
                "modified".yellow()
            };
            println!(
                "  {} {} ({} ops)",
                status,
                change.file_path,
                op_count
            );
        }
    }

    // Conflicts
    let conflicted = repo.list_conflicted_resources()?;
    if !conflicted.is_empty() {
        println!("\n{}:", "Unresolved conflicts".red().bold());
        for resource_id in &conflicted {
            let conflicts = repo.load_conflicts(resource_id)?;
            println!(
                "  {} {} ({} conflict(s))",
                "conflict".red(),
                resource_id,
                conflicts.len()
            );
        }
        println!(
            "\n  Use '{}' to resolve conflicts.",
            "dyna resolve <file>".bold()
        );
    }

    // Hint for new users
    if channel.patches.is_empty() && staged.is_empty() {
        println!("\n{}", "Hint: Use 'dyna add <file.json>' to stage a resource, then 'dyna commit -m \"message\"' to commit.".dimmed());
    }

    Ok(())
}
