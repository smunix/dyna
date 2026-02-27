//! `dyna status` command implementation.
//!
//! Shows:
//! - Current channel and HEAD info
//! - Remote sync status
//! - Staged changes
//! - Non-staged (untracked or modified) JSON files in the working directory
//! - Unresolved conflicts

use anyhow::Result;
use colored::Colorize;
use std::collections::HashSet;
use std::path::Path;

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
    println!("  Total patches: {}", channel.patches.len());

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
    let staged_resource_ids: HashSet<String> =
        staged.iter().map(|s| s.resource_id.clone()).collect();
    let staged_file_paths: HashSet<String> =
        staged.iter().map(|s| s.file_path.clone()).collect();

    if staged.is_empty() {
        println!("\n{}", "No staged changes.".dimmed());
    } else {
        println!("\n{}:", "Staged changes".green().bold());
        for change in &staged {
            let op_count = change.operations.len();
            let status = if change.previous.is_none() {
                "new     ".green()
            } else {
                "modified".yellow()
            };
            println!("  {} {} ({} ops)", status, change.file_path, op_count);
        }
    }

    // Non-staged files: scan the working directory for JSON files that are
    // either untracked (no snapshot) or modified (content differs from snapshot).
    let json_files = find_json_files(&repo.work_dir, &repo.dyna_dir)?;
    let snapshots = repo.load_all_snapshots()?;

    let mut untracked: Vec<String> = Vec::new();
    let mut modified_unstaged: Vec<String> = Vec::new();
    let mut up_to_date: Vec<String> = Vec::new();

    for json_path in &json_files {
        let abs_path = repo.work_dir.join(json_path);
        let resource_id = Repository::resource_id_from_path(&abs_path);

        // Skip files that are already staged
        if staged_resource_ids.contains(&resource_id) || staged_file_paths.contains(json_path) {
            continue;
        }

        if let Some(snapshot) = snapshots.get(&resource_id) {
            // Known resource — check if the file has been modified
            match std::fs::read_to_string(&abs_path) {
                Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(current_value) => {
                        if &current_value != snapshot {
                            modified_unstaged.push(json_path.clone());
                        } else {
                            up_to_date.push(json_path.clone());
                        }
                    }
                    Err(_) => {
                        // Not valid JSON — skip silently
                    }
                },
                Err(_) => {}
            }
        } else {
            // No snapshot exists — this is an untracked file
            untracked.push(json_path.clone());
        }
    }

    if !modified_unstaged.is_empty() {
        println!(
            "\n{} (use \"dyna add <file>\" to stage):",
            "Modified but not staged".yellow().bold()
        );
        for path in &modified_unstaged {
            println!("  {} {}", "modified".yellow(), path);
        }
    }

    if !untracked.is_empty() {
        println!(
            "\n{} (use \"dyna add <file>\" to track):",
            "Untracked files".red().bold()
        );
        for path in &untracked {
            println!("  {} {}", "untracked".red(), path);
        }
    }

    if modified_unstaged.is_empty() && untracked.is_empty() && staged.is_empty() {
        println!("\n{}", "Working directory clean.".green());
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
    if channel.patches.is_empty() && staged.is_empty() && untracked.is_empty() {
        println!(
            "\n{}",
            "Hint: Place .json files in this directory, then use 'dyna add <file>' to stage them."
                .dimmed()
        );
    }

    Ok(())
}

/// Recursively find all `.json` files in the working directory, excluding the
/// `.dyna/` metadata directory. Returns paths relative to `work_dir`.
fn find_json_files(work_dir: &Path, dyna_dir: &Path) -> Result<Vec<String>> {
    let mut results = Vec::new();
    collect_json_files(work_dir, work_dir, dyna_dir, &mut results)?;
    results.sort();
    Ok(results)
}

fn collect_json_files(
    base: &Path,
    dir: &Path,
    dyna_dir: &Path,
    results: &mut Vec<String>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    // Skip the .dyna directory
    if dir.starts_with(dyna_dir) {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Skip hidden directories and .dyna
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            collect_json_files(base, &path, dyna_dir, results)?;
        } else if path.extension().map_or(false, |ext| ext == "json") {
            // Compute relative path
            if let Ok(relative) = path.strip_prefix(base) {
                results.push(relative.display().to_string());
            }
        }
    }

    Ok(())
}
