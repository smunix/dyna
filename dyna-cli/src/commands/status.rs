//! `dyna status` command implementation.
//!
//! Shows:
//! - Current channel, working changeset, and HEAD info
//! - Remote sync status
//! - Staged changes (including deletions)
//! - Staged files with unstaged modifications
//! - Non-staged (untracked or modified) JSON files in the working directory
//! - Deleted tracked files (snapshots whose filesystem files are missing)
//! - Unresolved conflicts
//!
//! All filesystem I/O goes through the Repository's VFS abstraction.

use anyhow::Result;
use colored::Colorize;
use itertools::{izip, Itertools};
use std::collections::HashSet;

use crate::repository::Repository;

pub async fn execute() -> Result<()> {
    let repo = Repository::find_current()?;
    let config = repo.load_config()?;
    let channel_name = repo.current_channel_name()?;
    let channel = repo.load_channel(&channel_name)?;

    // Header
    println!("On channel {}", channel_name.bold().cyan());

    // Working changeset (the @ changeset)
    repo.working_change_id()?
        .map(|wc_id| {
            let short = &wc_id[..std::cmp::min(wc_id.len(), 8)];
            println!("  Working changeset: {} (@)", short.yellow().bold());
        })
        .unwrap_or_else(|| println!("  Working changeset: (none)"));

    channel
        .head_change_id
        .as_ref()
        .map(|head| println!("  HEAD: {}", &head[..std::cmp::min(head.len(), 8)]))
        .unwrap_or_else(|| println!("  HEAD: (no changesets yet)"));

    println!("  Total changesets: {}", channel.changesets.len());

    // Remote info
    config
        .remote_url
        .as_ref()
        .map(|url| -> Result<()> {
            let sync_state = repo.load_sync_state()?;
            let unpushed_count = sync_state
                .remote_heads
                .get(&channel_name)
                .and_then(|rh| {
                    izip!(&channel.changesets)
                        .position(|id| id == rh)
                        .map(|pos| channel.changesets.len() - pos - 1)
                })
                .unwrap_or(channel.changesets.len());

            (unpushed_count == 0)
                .then(|| println!("  Remote: {} (up-to-date)", url))
                .unwrap_or_else(|| {
                    println!(
                        "  Remote: {} ({} unpushed changeset(s))",
                        url,
                        unpushed_count.to_string().yellow()
                    );
                });
            Ok(())
        })
        .transpose()?
        .unwrap_or_else(|| println!("  Remote: (not configured)"));

    // Staged changes
    let staged = repo.load_staged_changes()?;
    let staged_resource_ids: HashSet<String> =
        izip!(&staged).map(|s| s.resource_id.clone()).collect();
    let staged_file_paths: HashSet<String> =
        izip!(&staged).map(|s| s.file_path.clone()).collect();

    staged
        .is_empty()
        .then(|| println!("\n{}", "No staged changes.".dimmed()))
        .unwrap_or_else(|| {
            println!("\n{}:", "Staged changes".green().bold());
            izip!(&staged).for_each(|change| {
                let status = change
                    .previous
                    .as_ref()
                    .map(|_| {
                        change
                            .current
                            .is_null()
                            .then(|| "deleted ".red())
                            .unwrap_or_else(|| "modified".yellow())
                    })
                    .unwrap_or_else(|| "new     ".green());
                println!("  {} {} ({} ops)", status, change.file_path, change.operations.len());
            });
        });

    // Detect unstaged modifications on already-staged files: compare the
    // staged `current` value against what is now on disk via VFS.
    let unstaged_on_staged: Vec<&dyna_core::models::StagedChange> = izip!(&staged)
        .filter(|change| {
            if change.current.is_null() {
                return false;
            }
            repo.read_work_file(&change.file_path)
                .ok()
                .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
                .map_or(false, |disk_value| disk_value != change.current)
        })
        .collect_vec();

    (!unstaged_on_staged.is_empty()).then(|| {
        println!(
            "\n{} (use \"dyna add <file>\" to update the staged version):",
            "Staged files with unstaged modifications".yellow().bold()
        );
        izip!(&unstaged_on_staged)
            .for_each(|change| println!("  {} {}", "modified".yellow(), change.file_path));
    });

    // Non-staged files — use repo.list_work_json_files() via VFS
    let json_files = repo.list_work_json_files()?;
    let snapshots = repo.load_all_snapshots()?;

    // Collect all filesystem resource IDs for later deletion detection
    let filesystem_resource_ids: HashSet<String> = izip!(&json_files)
        .map(|json_path| repo.resource_id_from_relative(json_path))
        .collect();

    let (modified_unstaged, untracked): (Vec<String>, Vec<String>) = izip!(json_files)
        .filter(|json_path| {
            let resource_id = repo.resource_id_from_relative(json_path);
            !staged_resource_ids.contains(&resource_id) && !staged_file_paths.contains(json_path)
        })
        .fold(
            (Vec::new(), Vec::new()),
            |(mut modified, mut untracked), json_path| {
                let resource_id = repo.resource_id_from_relative(&json_path);

                snapshots
                    .get(&resource_id)
                    .map(|snapshot| {
                        repo.read_work_file(&json_path)
                            .ok()
                            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
                            .filter(|current_value| current_value != snapshot)
                            .map(|_| modified.push(json_path.clone()));
                    })
                    .unwrap_or_else(|| {
                        untracked.push(json_path);
                    });

                (modified, untracked)
            },
        );

    // Detect deleted tracked files: snapshots whose resource IDs have no
    // corresponding file on the filesystem and are not already staged.
    let deleted_tracked: Vec<(String, String)> = izip!(snapshots.keys().sorted())
        .filter(|resource_id| {
            !filesystem_resource_ids.contains(*resource_id)
                && !staged_resource_ids.contains(*resource_id)
        })
        .map(|resource_id| {
            let relative = repo.relative_path_for_resource_id(resource_id);
            (resource_id.clone(), relative)
        })
        .collect_vec();

    (!modified_unstaged.is_empty()).then(|| {
        println!(
            "\n{} (use \"dyna add <file>\" to stage):",
            "Modified but not staged".yellow().bold()
        );
        izip!(&modified_unstaged)
            .for_each(|path| println!("  {} {}", "modified".yellow(), path));
    });

    (!deleted_tracked.is_empty()).then(|| {
        println!(
            "\n{} (use \"dyna add --delete <file>\" to stage removal, or \"dyna restore <file>\" to recover):",
            "Deleted tracked files".red().bold()
        );
        izip!(&deleted_tracked)
            .for_each(|(_, path)| println!("  {} {}", "deleted ".red(), path));
    });

    (!untracked.is_empty()).then(|| {
        println!(
            "\n{} (use \"dyna add <file>\" to track):",
            "Untracked files".red().bold()
        );
        izip!(&untracked)
            .for_each(|path| println!("  {} {}", "untracked".red(), path));
    });

    (modified_unstaged.is_empty()
        && untracked.is_empty()
        && staged.is_empty()
        && deleted_tracked.is_empty())
    .then(|| println!("\n{}", "Working directory clean.".green()));

    // Conflicts
    let conflicted = repo.list_conflicted_resources()?;
    (!conflicted.is_empty()).then(|| -> Result<()> {
        println!("\n{}:", "Unresolved conflicts".red().bold());
        izip!(&conflicted).try_for_each(|resource_id| {
            repo.load_conflicts(resource_id).map(|conflicts| {
                println!(
                    "  {} {} ({} conflict(s))",
                    "conflict".red(),
                    resource_id,
                    conflicts.len()
                );
            })
        })?;
        println!(
            "\n  Use '{}' to resolve conflicts.",
            "dyna resolve <file>".bold()
        );
        Ok(())
    }).transpose()?;

    (channel.changesets.is_empty()
        && staged.is_empty()
        && untracked.is_empty()
        && deleted_tracked.is_empty())
    .then(|| {
        println!(
            "\n{}",
            "Hint: Place .json files in this directory, then use 'dyna add <file>' to stage them."
                .dimmed()
        );
    });

    Ok(())
}
