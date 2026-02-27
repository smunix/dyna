//! `dyna diff` command implementation.
//!
//! Shows detailed operations for staged files. Optionally filters to a
//! specific file path.
//!
//! Usage:
//!   dyna diff              — show diffs for all staged files
//!   dyna diff <file.json>  — show diff for a specific staged file

use anyhow::Result;
use colored::Colorize;
use dyna_common::models::PatchOperation;
use std::path::PathBuf;

use crate::repository::Repository;

pub async fn execute(path: Option<PathBuf>) -> Result<()> {
    let repo = Repository::find_current()?;
    let staged = repo.load_staged_changes()?;

    if staged.is_empty() {
        println!("{}", "No staged changes to diff.".dimmed());
        println!(
            "{}",
            "Use 'dyna add <file.json>' to stage changes first.".dimmed()
        );
        return Ok(());
    }

    // Optionally filter to a specific file
    let filtered: Vec<_> = if let Some(ref filter_path) = path {
        let filter_str = filter_path.display().to_string();
        let abs_filter = if filter_path.is_absolute() {
            filter_path.clone()
        } else {
            std::env::current_dir()?.join(filter_path)
        };
        let filter_resource_id = Repository::resource_id_from_path(&abs_filter);

        staged
            .iter()
            .filter(|s| s.file_path == filter_str || s.resource_id == filter_resource_id)
            .collect()
    } else {
        staged.iter().collect()
    };

    if filtered.is_empty() {
        if let Some(ref p) = path {
            println!(
                "No staged changes found for '{}'.",
                p.display().to_string().bold()
            );
        }
        return Ok(());
    }

    for (idx, change) in filtered.iter().enumerate() {
        if idx > 0 {
            println!("{}", "─".repeat(72).dimmed());
        }

        // Header
        let status_label = if change.previous.is_none() {
            "new file".green().bold()
        } else {
            "modified".yellow().bold()
        };
        println!(
            "{} {}  (resource: {})",
            status_label,
            change.file_path.bold(),
            change.resource_id.dimmed()
        );
        println!(
            "  {} operation(s):\n",
            change.operations.len().to_string().cyan()
        );

        // Print each operation in detail
        for (op_idx, op) in change.operations.iter().enumerate() {
            print_operation(op_idx + 1, op);
        }

        // Print a compact before/after summary for replace operations
        let replaces: Vec<_> = change
            .operations
            .iter()
            .filter(|op| matches!(op, PatchOperation::Replace { .. }))
            .collect();
        if !replaces.is_empty() && change.previous.is_some() {
            println!("\n  {}:", "Value changes".underline());
            for op in replaces {
                if let PatchOperation::Replace { path, value } = op {
                    let old_value = change
                        .previous
                        .as_ref()
                        .and_then(|prev| resolve_json_pointer(prev, path));
                    println!("    {}:", path.bold());
                    if let Some(old) = old_value {
                        println!(
                            "      {} {}",
                            "-".red(),
                            format_value_compact(&old).red()
                        );
                    }
                    println!(
                        "      {} {}",
                        "+".green(),
                        format_value_compact(value).green()
                    );
                }
            }
        }

        println!();
    }

    // Summary
    let total_ops: usize = filtered.iter().map(|s| s.operations.len()).sum();
    println!(
        "{}: {} file(s), {} total operation(s)",
        "Staged diff summary".bold(),
        filtered.len(),
        total_ops
    );

    Ok(())
}

/// Print a single patch operation with formatting.
fn print_operation(index: usize, op: &PatchOperation) {
    match op {
        PatchOperation::Add { path, value } => {
            println!(
                "  {}. {} {} {}",
                index.to_string().dimmed(),
                "ADD".green().bold(),
                path.bold(),
                "=".dimmed()
            );
            print_value_indented(value, 6);
        }
        PatchOperation::Remove { path } => {
            println!(
                "  {}. {} {}",
                index.to_string().dimmed(),
                "REMOVE".red().bold(),
                path.bold()
            );
        }
        PatchOperation::Replace { path, value } => {
            println!(
                "  {}. {} {} {}",
                index.to_string().dimmed(),
                "REPLACE".yellow().bold(),
                path.bold(),
                "=".dimmed()
            );
            print_value_indented(value, 6);
        }
        PatchOperation::Move { from, path } => {
            println!(
                "  {}. {} {} {} {}",
                index.to_string().dimmed(),
                "MOVE".blue().bold(),
                from.bold(),
                "->".dimmed(),
                path.bold()
            );
        }
        PatchOperation::Copy { from, path } => {
            println!(
                "  {}. {} {} {} {}",
                index.to_string().dimmed(),
                "COPY".blue().bold(),
                from.bold(),
                "->".dimmed(),
                path.bold()
            );
        }
        PatchOperation::Test { path, value } => {
            println!(
                "  {}. {} {} {}",
                index.to_string().dimmed(),
                "TEST".magenta().bold(),
                path.bold(),
                "==".dimmed()
            );
            print_value_indented(value, 6);
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

/// Format a JSON value compactly for inline display.
fn format_value_compact(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => format!("\"{}\"", s),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => {
            let s = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
            if s.len() > 80 {
                format!("{}...", &s[..77])
            } else {
                s
            }
        }
    }
}

/// Resolve a JSON Pointer (RFC 6901) against a JSON value.
fn resolve_json_pointer<'a>(
    value: &'a serde_json::Value,
    pointer: &str,
) -> Option<serde_json::Value> {
    if pointer == "/" || pointer.is_empty() {
        return Some(value.clone());
    }
    value.pointer(pointer).cloned()
}
