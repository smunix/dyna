//! `dyna log` command implementation.
//!
//! Displays the changeset history for the current channel. Changesets are
//! the primary log unit (not individual patches).
//!
//! Options:
//!   --verbose:     show patches and operations within each changeset
//!   --changeset:   inspect a single changeset in full detail
//!   --patches:     when used with --changeset, show detailed operations

use anyhow::{Result, bail};
use colored::Colorize;
use dyna_core::models::{Changeset, PatchOperation};
use itertools::Itertools;

use crate::repository::Repository;

pub async fn execute(
    count: usize,
    verbose: bool,
    changeset_id: Option<String>,
    show_patches: bool,
) -> Result<()> {
    let repo = Repository::find_current()?;
    let channel_name = repo.current_channel_name()?;

    // --changeset <id>: show a single changeset in detail
    if let Some(id_or_prefix) = changeset_id {
        return show_single_changeset(&repo, &id_or_prefix, show_patches);
    }

    let changesets = repo.load_channel_changesets(&channel_name)?;

    if changesets.is_empty() {
        println!("No changesets in channel '{}'.", channel_name);
        return Ok(());
    }

    let working_change = repo.working_change_id()?;

    println!(
        "Channel '{}' — {} changeset(s)\n",
        channel_name.bold().cyan(),
        changesets.len()
    );

    // Show the most recent `count` changesets (newest first)
    let start = changesets.len().saturating_sub(count);

    changesets[start..]
        .iter()
        .rev()
        .for_each(|cs| {
            print_changeset_summary(cs, &working_change);
            if verbose {
                print_changeset_patches(cs);
            }
            println!();
        });

    (start > 0).then(|| {
        println!(
            "  ... {} earlier changeset(s) not shown (use -n to show more)",
            start
        );
    });

    (!verbose).then(|| {
        println!(
            "{}",
            "Hint: Use 'dyna log --verbose' to see patches and operations.".dimmed()
        );
        println!(
            "{}",
            "      Use 'dyna log --changeset <id>' to inspect a single changeset.".dimmed()
        );
    });

    Ok(())
}

/// Print a summary of a changeset, with DAG indicators.
fn print_changeset_summary(cs: &Changeset, working_change: &Option<String>) {
    let is_working = working_change
        .as_ref()
        .map_or(false, |wc| wc == &cs.change_id);

    let marker = is_working
        .then(|| "@".bold().green().to_string())
        .unwrap_or_else(|| "○".to_string());

    let immutable_flag = cs
        .immutable
        .then(|| " [immutable]".dimmed().to_string())
        .unwrap_or_default();

    let empty_flag = cs
        .empty
        .then(|| " (empty)".dimmed().to_string())
        .unwrap_or_default();

    let bookmarks = cs
        .bookmarks
        .is_empty()
        .then(String::new)
        .unwrap_or_else(|| format!(" {}", cs.bookmarks.join(" ").magenta()));

    println!(
        "{}  {} {} {} {}{}{}{}",
        marker,
        cs.short_change_id().yellow().bold(),
        cs.author.cyan(),
        cs.created_at.format("%Y-%m-%d %H:%M:%S"),
        cs.short_commit_hash(),
        bookmarks,
        immutable_flag,
        empty_flag,
    );

    // Message
    cs.message
        .is_empty()
        .then(|| println!("│  {}", "(no description set)".dimmed()))
        .unwrap_or_else(|| println!("│  {}", cs.message));

    // Resources affected
    let resources = cs.affected_resources();
    (!resources.is_empty()).then(|| {
        println!(
            "│  {} resource(s): {}",
            resources.len(),
            resources.join(", ").dimmed()
        );
    });

    // Parents
    (!cs.parents.is_empty()).then(|| {
        let parent_strs = cs
            .parents
            .iter()
            .map(|p| p[..std::cmp::min(p.len(), 8)].to_string())
            .join(", ");
        println!("│  parent(s): {}", parent_strs.dimmed());
    });
}

/// Print the patches within a changeset (verbose mode).
fn print_changeset_patches(cs: &Changeset) {
    if cs.patches.is_empty() {
        return;
    }

    println!(
        "│  ── {} patch(es), {} operation(s) ──",
        cs.patches.len(),
        cs.total_operations()
    );

    cs.patches.iter().enumerate().for_each(|(i, patch)| {
        let short_hash = &patch.hash[7..std::cmp::min(patch.hash.len(), 19)];
        println!(
            "│  patch {}: [{}] {} ({} ops)",
            (i + 1).to_string().dimmed(),
            short_hash.yellow(),
            patch.target_resource.bold(),
            patch.operations.len()
        );

        patch
            .operations
            .iter()
            .enumerate()
            .for_each(|(op_idx, op)| {
                print_operation(op_idx + 1, op, "│    ");
            });
    });
}

/// Show a single changeset in full detail.
fn show_single_changeset(
    repo: &Repository,
    id_or_prefix: &str,
    show_patches: bool,
) -> Result<()> {
    // Try exact match first, then prefix match
    let cs = repo
        .load_changeset(id_or_prefix)
        .or_else(|_| {
            repo.find_changeset_by_prefix(id_or_prefix)
                .and_then(|matches| match matches.len() {
                    0 => bail!("No changeset found matching '{}'", id_or_prefix),
                    1 => Ok(matches.into_iter().next().unwrap()),
                    n => {
                        matches.iter().for_each(|m| {
                            println!("  {} - {}", m.short_change_id(), m.message);
                        });
                        bail!(
                            "Ambiguous prefix '{}' matches {} changesets. Please provide a longer prefix.",
                            id_or_prefix,
                            n
                        );
                    }
                })
        })?;

    println!("{}", "═".repeat(72).dimmed());
    println!("{}", "Changeset Detail".bold());
    println!("{}", "═".repeat(72).dimmed());

    // Metadata fields via iterator of (label, value) tuples
    [
        ("change_id", cs.change_id.yellow().bold().to_string()),
        ("commit_hash", cs.commit_hash.clone()),
        ("author", cs.author.cyan().to_string()),
        ("created", cs.created_at.format("%Y-%m-%d %H:%M:%S").to_string()),
        ("updated", cs.updated_at.format("%Y-%m-%d %H:%M:%S").to_string()),
        ("immutable", cs.immutable.to_string()),
        ("empty", cs.empty.to_string()),
    ]
    .iter()
    .for_each(|(label, value)| println!("  {:12} {}", format!("{}:", label), value));

    cs.message
        .is_empty()
        .then(|| println!("  {:12} {}", "message:", "(no description set)".dimmed()))
        .unwrap_or_else(|| println!("  {:12} {}", "message:", cs.message));

    cs.parents
        .is_empty()
        .then(|| println!("  {:12} {}", "parents:", "(root changeset)".dimmed()))
        .unwrap_or_else(|| {
            cs.parents
                .iter()
                .enumerate()
                .for_each(|(i, parent)| println!("  parent[{}]:   {}", i, parent));
        });

    (!cs.bookmarks.is_empty()).then(|| {
        println!("  {:12} {}", "bookmarks:", cs.bookmarks.join(", ").magenta());
    });

    let resources = cs.affected_resources();
    println!(
        "\n  Resources affected: {} ({})",
        resources.len(),
        resources.join(", ")
    );
    println!(
        "  Patches: {} ({} total operations)",
        cs.patches.len(),
        cs.total_operations()
    );

    // Always list patches
    println!("\n  {}:", "Patches in this changeset".underline().bold());
    cs.patches.iter().enumerate().for_each(|(i, patch)| {
        let short_hash = &patch.hash[7..std::cmp::min(patch.hash.len(), 19)];
        println!(
            "    {}. [{}] {} — {} operation(s)",
            i + 1,
            short_hash.yellow(),
            patch.target_resource.bold(),
            patch.operations.len()
        );

        // If --patches flag, show detailed operations
        if show_patches {
            patch.parent_snapshot.as_ref().map(|parent| {
                println!(
                    "       {}: {}",
                    "parent snapshot".dimmed(),
                    truncate_json(parent, 80)
                );
            });
            patch.result_snapshot.as_ref().map(|result| {
                println!(
                    "       {}: {}",
                    "result snapshot".dimmed(),
                    truncate_json(result, 80)
                );
            });

            patch
                .operations
                .iter()
                .enumerate()
                .for_each(|(op_idx, op)| {
                    print_operation(op_idx + 1, op, "       ");
                });
        }
    });

    // Verify integrity
    cs.verify()
        .then(|| println!("\n  Integrity: {}", "VALID".green().bold()))
        .unwrap_or_else(|| {
            println!(
                "\n  Integrity: {}",
                "INVALID (commit hash mismatch!)".red().bold()
            );
        });

    println!("{}", "═".repeat(72).dimmed());

    Ok(())
}

/// Print a single patch operation with formatting.
fn print_operation(index: usize, op: &PatchOperation, prefix: &str) {
    let idx = index.to_string().dimmed();
    match op {
        PatchOperation::Add { path, value } => println!(
            "{}{}. {} {} = {}",
            prefix, idx, "ADD".green().bold(), path.bold(), truncate_json(value, 60)
        ),
        PatchOperation::Remove { path } => println!(
            "{}{}. {} {}",
            prefix, idx, "REMOVE".red().bold(), path.bold()
        ),
        PatchOperation::Replace { path, value } => println!(
            "{}{}. {} {} = {}",
            prefix, idx, "REPLACE".yellow().bold(), path.bold(), truncate_json(value, 60)
        ),
        PatchOperation::Move { from, path } => println!(
            "{}{}. {} {} -> {}",
            prefix, idx, "MOVE".blue().bold(), from.bold(), path.bold()
        ),
        PatchOperation::Copy { from, path } => println!(
            "{}{}. {} {} -> {}",
            prefix, idx, "COPY".blue().bold(), from.bold(), path.bold()
        ),
        PatchOperation::Test { path, value } => println!(
            "{}{}. {} {} == {}",
            prefix, idx, "TEST".magenta().bold(), path.bold(), truncate_json(value, 60)
        ),
    }
}

/// Truncate a JSON value for display.
fn truncate_json(value: &serde_json::Value, max_len: usize) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "???".into())
        .chars()
        .take(max_len)
        .collect::<String>()
        .pipe_if_longer(max_len, |s| format!("{}...", s))
}

/// Extension trait for conditional string transformation.
trait PipeIfLonger {
    fn pipe_if_longer(self, max: usize, f: impl FnOnce(String) -> String) -> String;
}

impl PipeIfLonger for String {
    fn pipe_if_longer(self, max: usize, f: impl FnOnce(String) -> String) -> String {
        if self.len() >= max { f(self) } else { self }
    }
}
