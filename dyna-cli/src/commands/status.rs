//! `dyna status` command implementation.
//!
//! Shows:
//! - Current channel, working changeset, and HEAD info
//! - Remote sync status
//! - Staged changes
//! - Non-staged (untracked or modified) JSON files in the working directory
//! - Unresolved conflicts

use anyhow::Result;
use colored::Colorize;
use itertools::{izip, Itertools};
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
                    .is_none()
                    .then(|| "new     ".green())
                    .unwrap_or_else(|| "modified".yellow());
                println!("  {} {} ({} ops)", status, change.file_path, change.operations.len());
            });
        });

    // Non-staged files — partition into modified and untracked via fold
    let json_files = find_json_files(&repo.work_dir, &repo.dyna_dir)?;
    let snapshots = repo.load_all_snapshots()?;

    let (modified_unstaged, untracked): (Vec<String>, Vec<String>) = izip!(json_files)
        .filter(|json_path| {
            let abs_path = repo.work_dir.join(json_path);
            let resource_id = repo.resource_id_from_path(&abs_path);
            !staged_resource_ids.contains(&resource_id) && !staged_file_paths.contains(json_path)
        })
        .fold(
            (Vec::new(), Vec::new()),
            |(mut modified, mut untracked), json_path| {
                let abs_path = repo.work_dir.join(&json_path);
                let resource_id = repo.resource_id_from_path(&abs_path);

                snapshots
                    .get(&resource_id)
                    .map(|snapshot| {
                        std::fs::read_to_string(&abs_path)
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

    (!modified_unstaged.is_empty()).then(|| {
        println!(
            "\n{} (use \"dyna add <file>\" to stage):",
            "Modified but not staged".yellow().bold()
        );
        izip!(&modified_unstaged)
            .for_each(|path| println!("  {} {}", "modified".yellow(), path));
    });

    (!untracked.is_empty()).then(|| {
        println!(
            "\n{} (use \"dyna add <file>\" to track):",
            "Untracked files".red().bold()
        );
        izip!(&untracked)
            .for_each(|path| println!("  {} {}", "untracked".red(), path));
    });

    (modified_unstaged.is_empty() && untracked.is_empty() && staged.is_empty())
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

    (channel.changesets.is_empty() && staged.is_empty() && untracked.is_empty()).then(|| {
        println!(
            "\n{}",
            "Hint: Place .json files in this directory, then use 'dyna add <file>' to stage them."
                .dimmed()
        );
    });

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
    if !dir.is_dir() || dir.starts_with(dyna_dir) {
        return Ok(());
    }

    std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .sorted()
        .try_for_each(|path| {
            if path.is_dir() {
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .filter(|name| !name.starts_with('.'))
                    .map(|_| collect_json_files(base, &path, dyna_dir, results))
                    .unwrap_or(Ok(()))
            } else {
                path.extension()
                    .filter(|ext| *ext == "json")
                    .and_then(|_| path.strip_prefix(base).ok())
                    .map(|relative| results.push(relative.display().to_string()));
                Ok(())
            }
        })
}
