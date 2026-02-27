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

    // Load all changesets for the current channel
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
    let start = if changesets.len() > count {
        changesets.len() - count
    } else {
        0
    };

    for cs in changesets[start..].iter().rev() {
        print_changeset_summary(cs, &working_change);

        if verbose {
            print_changeset_patches(cs);
        }

        println!();
    }

    if start > 0 {
        println!(
            "  ... {} earlier changeset(s) not shown (use -n to show more)",
            start
        );
    }

    if !verbose {
        println!(
            "{}",
            "Hint: Use 'dyna log --verbose' to see patches and operations.".dimmed()
        );
        println!(
            "{}",
            "      Use 'dyna log --changeset <id>' to inspect a single changeset.".dimmed()
        );
    }

    Ok(())
}

/// Print a summary of a changeset, with DAG indicators.
fn print_changeset_summary(cs: &Changeset, working_change: &Option<String>) {
    let is_working = working_change
        .as_ref()
        .map_or(false, |wc| wc == &cs.change_id);

    let marker = if is_working {
        "@".bold().green().to_string()
    } else {
        "○".to_string()
    };

    let immutable_flag = if cs.immutable {
        " [immutable]".dimmed().to_string()
    } else {
        String::new()
    };

    let empty_flag = if cs.empty {
        " (empty)".dimmed().to_string()
    } else {
        String::new()
    };

    // Bookmarks
    let bookmarks = if cs.bookmarks.is_empty() {
        String::new()
    } else {
        format!(" {}", cs.bookmarks.join(" ").magenta())
    };

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
    if !cs.message.is_empty() {
        println!("│  {}", cs.message);
    } else {
        println!("│  {}", "(no description set)".dimmed());
    }

    // Resources affected
    let resources = cs.affected_resources();
    if !resources.is_empty() {
        println!(
            "│  {} resource(s): {}",
            resources.len(),
            resources.join(", ").dimmed()
        );
    }

    // Parents
    if !cs.parents.is_empty() {
        let parent_strs: Vec<String> = cs
            .parents
            .iter()
            .map(|p| p[..std::cmp::min(p.len(), 8)].to_string())
            .collect();
        println!("│  parent(s): {}", parent_strs.join(", ").dimmed());
    }
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

    for (i, patch) in cs.patches.iter().enumerate() {
        let short_hash = &patch.hash[7..std::cmp::min(patch.hash.len(), 19)];
        println!(
            "│  patch {}: [{}] {} ({} ops)",
            (i + 1).to_string().dimmed(),
            short_hash.yellow(),
            patch.target_resource.bold(),
            patch.operations.len()
        );

        for (op_idx, op) in patch.operations.iter().enumerate() {
            print_operation(op_idx + 1, op, "│    ");
        }
    }
}

/// Show a single changeset in full detail.
fn show_single_changeset(
    repo: &Repository,
    id_or_prefix: &str,
    show_patches: bool,
) -> Result<()> {
    // Try exact match first
    let cs = match repo.load_changeset(id_or_prefix) {
        Ok(cs) => cs,
        Err(_) => {
            // Try prefix match
            let matches = repo.find_changeset_by_prefix(id_or_prefix)?;
            match matches.len() {
                0 => bail!("No changeset found matching '{}'", id_or_prefix),
                1 => matches.into_iter().next().unwrap(),
                n => {
                    println!(
                        "{} Ambiguous prefix '{}' matches {} changesets:",
                        "warning:".yellow().bold(),
                        id_or_prefix,
                        n
                    );
                    for m in &matches {
                        println!("  {} - {}", m.short_change_id(), m.message);
                    }
                    bail!("Please provide a longer prefix to disambiguate.");
                }
            }
        }
    };

    println!("{}", "═".repeat(72).dimmed());
    println!("{}", "Changeset Detail".bold());
    println!("{}", "═".repeat(72).dimmed());
    println!("  change_id:   {}", cs.change_id.yellow().bold());
    println!("  commit_hash: {}", cs.commit_hash);
    println!("  author:      {}", cs.author.cyan());
    println!(
        "  created:     {}",
        cs.created_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!(
        "  updated:     {}",
        cs.updated_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!("  immutable:   {}", cs.immutable);
    println!("  empty:       {}", cs.empty);

    if !cs.message.is_empty() {
        println!("  message:     {}", cs.message);
    } else {
        println!("  message:     {}", "(no description set)".dimmed());
    }

    if cs.parents.is_empty() {
        println!("  parents:     {}", "(root changeset)".dimmed());
    } else {
        for (i, parent) in cs.parents.iter().enumerate() {
            println!("  parent[{}]:   {}", i, parent);
        }
    }

    if !cs.bookmarks.is_empty() {
        println!("  bookmarks:   {}", cs.bookmarks.join(", ").magenta());
    }

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
    for (i, patch) in cs.patches.iter().enumerate() {
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
            if let Some(ref parent) = patch.parent_snapshot {
                println!(
                    "       {}: {}",
                    "parent snapshot".dimmed(),
                    truncate_json(parent, 80)
                );
            }
            if let Some(ref result) = patch.result_snapshot {
                println!(
                    "       {}: {}",
                    "result snapshot".dimmed(),
                    truncate_json(result, 80)
                );
            }

            for (op_idx, op) in patch.operations.iter().enumerate() {
                print_operation(op_idx + 1, op, "       ");
            }
        }
    }

    // Verify integrity
    if cs.verify() {
        println!("\n  Integrity: {}", "VALID".green().bold());
    } else {
        println!(
            "\n  Integrity: {}",
            "INVALID (commit hash mismatch!)".red().bold()
        );
    }

    println!("{}", "═".repeat(72).dimmed());

    Ok(())
}

/// Print a single patch operation with formatting.
fn print_operation(index: usize, op: &PatchOperation, prefix: &str) {
    match op {
        PatchOperation::Add { path, value } => {
            println!(
                "{}{}. {} {} = {}",
                prefix,
                index.to_string().dimmed(),
                "ADD".green().bold(),
                path.bold(),
                truncate_json(value, 60)
            );
        }
        PatchOperation::Remove { path } => {
            println!(
                "{}{}. {} {}",
                prefix,
                index.to_string().dimmed(),
                "REMOVE".red().bold(),
                path.bold()
            );
        }
        PatchOperation::Replace { path, value } => {
            println!(
                "{}{}. {} {} = {}",
                prefix,
                index.to_string().dimmed(),
                "REPLACE".yellow().bold(),
                path.bold(),
                truncate_json(value, 60)
            );
        }
        PatchOperation::Move { from, path } => {
            println!(
                "{}{}. {} {} -> {}",
                prefix,
                index.to_string().dimmed(),
                "MOVE".blue().bold(),
                from.bold(),
                path.bold()
            );
        }
        PatchOperation::Copy { from, path } => {
            println!(
                "{}{}. {} {} -> {}",
                prefix,
                index.to_string().dimmed(),
                "COPY".blue().bold(),
                from.bold(),
                path.bold()
            );
        }
        PatchOperation::Test { path, value } => {
            println!(
                "{}{}. {} {} == {}",
                prefix,
                index.to_string().dimmed(),
                "TEST".magenta().bold(),
                path.bold(),
                truncate_json(value, 60)
            );
        }
    }
}

/// Truncate a JSON value for display.
fn truncate_json(value: &serde_json::Value, max_len: usize) -> String {
    let s = serde_json::to_string(value).unwrap_or_else(|_| "???".into());
    if s.len() > max_len {
        format!("{}...", &s[..max_len])
    } else {
        s
    }
}
