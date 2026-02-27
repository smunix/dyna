//! `dyna log` command implementation.
//!
//! Supports:
//! - `dyna log` — show recent patch history (summary)
//! - `dyna log --verbose` — show patch history with detailed operations
//! - `dyna log --patch <hash>` — show detailed operations for a specific patch

use anyhow::{Result, bail};
use colored::Colorize;
use dyna_common::models::PatchOperation;

use crate::repository::Repository;

pub async fn execute(count: usize, verbose: bool, patch_id: Option<String>) -> Result<()> {
    let repo = Repository::find_current()?;

    // ------------------------------------------------------------------
    // Single patch detail mode: `dyna log --patch <hash>`
    // ------------------------------------------------------------------
    if let Some(ref id) = patch_id {
        return show_patch_detail(&repo, id);
    }

    // ------------------------------------------------------------------
    // Channel log mode: `dyna log [-n N] [--verbose]`
    // ------------------------------------------------------------------
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
    let patches_to_show: Vec<_> = channel.patches.iter().rev().take(count).collect();

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

                // Verbose mode: print detailed operations
                if verbose {
                    println!();
                    for (op_idx, op) in patch.operations.iter().enumerate() {
                        print_operation(op_idx + 1, op, 15);
                    }
                }

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

    if !verbose {
        println!(
            "{}",
            "Hint: Use 'dyna log --verbose' to see detailed operations for each patch.".dimmed()
        );
    }

    Ok(())
}

/// Show detailed information for a single patch, identified by its hash or
/// a prefix of its hash.
fn show_patch_detail(repo: &Repository, patch_id: &str) -> Result<()> {
    // Try to find the patch by exact hash or prefix match
    let patch = match repo.load_patch(patch_id) {
        Ok(p) => p,
        Err(_) => {
            // Try prefix matching against all known patches
            match find_patch_by_prefix(repo, patch_id)? {
                Some(p) => p,
                None => {
                    bail!(
                        "Patch '{}' not found. Use 'dyna log' to list available patches.",
                        patch_id
                    );
                }
            }
        }
    };

    // Full header
    println!("{}", "═".repeat(72).dimmed());
    println!("Patch {}", patch.hash.yellow().bold());
    println!("{}", "═".repeat(72).dimmed());
    println!("  Author:   {}", patch.author);
    println!(
        "  Date:     {}",
        patch.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("  Resource: {}", patch.target_resource.bold());
    println!("  Message:  {}", patch.message);

    if !patch.dependencies.is_empty() {
        println!("\n  {}:", "Dependencies".underline());
        for dep in &patch.dependencies {
            println!("    {}", dep.dimmed());
        }
    }

    // Operations
    println!(
        "\n  {} ({}):",
        "Operations".underline().bold(),
        format!("{} total", patch.operations.len()).dimmed()
    );
    println!();

    for (op_idx, op) in patch.operations.iter().enumerate() {
        print_operation(op_idx + 1, op, 4);
    }

    // Parent and result snapshots
    if let Some(ref parent) = patch.parent_snapshot {
        println!("\n  {}:", "Parent snapshot".underline());
        print_value_indented(parent, 4);
    }

    if let Some(ref result) = patch.result_snapshot {
        println!("\n  {}:", "Result snapshot".underline());
        print_value_indented(result, 4);
    }

    println!("\n{}", "═".repeat(72).dimmed());

    Ok(())
}

/// Find a patch by hash prefix. Searches all locally stored patches.
fn find_patch_by_prefix(
    repo: &Repository,
    prefix: &str,
) -> Result<Option<dyna_common::models::Patch>> {
    let channel_name = repo.current_channel_name()?;
    let channel = repo.load_channel(&channel_name)?;

    // Normalize: allow users to omit the "sha256:" prefix
    let search_prefix = if prefix.starts_with("sha256:") {
        prefix.to_string()
    } else {
        format!("sha256:{}", prefix)
    };

    let mut matches = Vec::new();
    for hash in &channel.patches {
        if hash.starts_with(&search_prefix) {
            if let Ok(patch) = repo.load_patch(hash) {
                matches.push(patch);
            }
        }
    }

    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches.into_iter().next().unwrap())),
        n => {
            eprintln!(
                "{} Ambiguous patch prefix '{}' matches {} patches:",
                "warning:".yellow().bold(),
                prefix,
                n
            );
            for m in &matches {
                eprintln!("  {}", m.hash);
            }
            eprintln!("Please provide a longer prefix to disambiguate.");
            Ok(None)
        }
    }
}

/// Print a single patch operation with formatting.
fn print_operation(index: usize, op: &PatchOperation, indent: usize) {
    let pad = " ".repeat(indent);
    match op {
        PatchOperation::Add { path, value } => {
            println!(
                "{}{}. {} {} {}",
                pad,
                index.to_string().dimmed(),
                "ADD".green().bold(),
                path.bold(),
                "=".dimmed()
            );
            print_value_indented(value, indent + 4);
        }
        PatchOperation::Remove { path } => {
            println!(
                "{}{}. {} {}",
                pad,
                index.to_string().dimmed(),
                "REMOVE".red().bold(),
                path.bold()
            );
        }
        PatchOperation::Replace { path, value } => {
            println!(
                "{}{}. {} {} {}",
                pad,
                index.to_string().dimmed(),
                "REPLACE".yellow().bold(),
                path.bold(),
                "=".dimmed()
            );
            print_value_indented(value, indent + 4);
        }
        PatchOperation::Move { from, path } => {
            println!(
                "{}{}. {} {} {} {}",
                pad,
                index.to_string().dimmed(),
                "MOVE".blue().bold(),
                from.bold(),
                "->".dimmed(),
                path.bold()
            );
        }
        PatchOperation::Copy { from, path } => {
            println!(
                "{}{}. {} {} {} {}",
                pad,
                index.to_string().dimmed(),
                "COPY".blue().bold(),
                from.bold(),
                "->".dimmed(),
                path.bold()
            );
        }
        PatchOperation::Test { path, value } => {
            println!(
                "{}{}. {} {} {}",
                pad,
                index.to_string().dimmed(),
                "TEST".magenta().bold(),
                path.bold(),
                "==".dimmed()
            );
            print_value_indented(value, indent + 4);
        }
    }
}

/// Print a JSON value with indentation.
fn print_value_indented(value: &serde_json::Value, indent: usize) {
    let formatted = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    let prefix = " ".repeat(indent);
    for line in formatted.lines() {
        println!("{}{}", prefix, line);
    }
}
