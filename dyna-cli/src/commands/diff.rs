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
use dyna_core::models::PatchOperation;
use itertools::{izip, Itertools};
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
    let filtered = path
        .as_ref()
        .map(|filter_path| {
            let filter_str = filter_path.display().to_string();
            let abs_filter = filter_path
                .is_absolute()
                .then(|| filter_path.clone())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join(filter_path));
            let filter_resource_id = repo.resource_id_from_path(&abs_filter);

            izip!(&staged)
                .filter(|s| s.file_path == filter_str || s.resource_id == filter_resource_id)
                .collect_vec()
        })
        .unwrap_or_else(|| izip!(&staged).collect_vec());

    if filtered.is_empty() {
        path.as_ref().map(|p| {
            println!(
                "No staged changes found for '{}'.",
                p.display().to_string().bold()
            );
        });
        return Ok(());
    }

    izip!(&filtered).enumerate().for_each(|(idx, change)| {
        (idx > 0).then(|| println!("{}", "─".repeat(72).dimmed()));

        // Header
        let status_label = change
            .previous
            .is_none()
            .then(|| "new file".green().bold())
            .unwrap_or_else(|| "modified".yellow().bold());

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

        // Print each operation
        izip!(&change.operations)
            .enumerate()
            .for_each(|(op_idx, op)| print_operation(op_idx + 1, op));

        // Print a compact before/after summary for replace operations
        let replaces = izip!(&change.operations)
            .filter(|op| matches!(op, PatchOperation::Replace { .. }))
            .collect_vec();

        (!replaces.is_empty() && change.previous.is_some()).then(|| {
            println!("\n  {}:", "Value changes".underline());
            izip!(&replaces).for_each(|op| {
                if let PatchOperation::Replace { path, value } = op {
                    let old_value = change
                        .previous
                        .as_ref()
                        .and_then(|prev| resolve_json_pointer(prev, path));

                    println!("    {}:", path.bold());
                    old_value.map(|old| {
                        println!("      {} {}", "-".red(), format_value_compact(&old).red());
                    });
                    println!(
                        "      {} {}",
                        "+".green(),
                        format_value_compact(value).green()
                    );
                }
            });
        });

        println!();
    });

    // Summary
    let total_ops: usize = izip!(&filtered).map(|s| s.operations.len()).sum();
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
    let idx = index.to_string().dimmed();
    match op {
        PatchOperation::Add { path, value } => {
            println!("  {}. {} {} {}", idx, "ADD".green().bold(), path.bold(), "=".dimmed());
            print_value_indented(value, 6);
        }
        PatchOperation::Remove { path } => {
            println!("  {}. {} {}", idx, "REMOVE".red().bold(), path.bold());
        }
        PatchOperation::Replace { path, value } => {
            println!("  {}. {} {} {}", idx, "REPLACE".yellow().bold(), path.bold(), "=".dimmed());
            print_value_indented(value, 6);
        }
        PatchOperation::Move { from, path } => {
            println!("  {}. {} {} {} {}", idx, "MOVE".blue().bold(), from.bold(), "->".dimmed(), path.bold());
        }
        PatchOperation::Copy { from, path } => {
            println!("  {}. {} {} {} {}", idx, "COPY".blue().bold(), from.bold(), "->".dimmed(), path.bold());
        }
        PatchOperation::Test { path, value } => {
            println!("  {}. {} {} {}", idx, "TEST".magenta().bold(), path.bold(), "==".dimmed());
            print_value_indented(value, 6);
        }
    }
}

/// Print a JSON value with indentation.
fn print_value_indented(value: &serde_json::Value, indent: usize) {
    let prefix = " ".repeat(indent);
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|_| value.to_string())
        .lines()
        .for_each(|line| println!("{}{}", prefix, line));
}

/// Format a JSON value compactly for inline display.
fn format_value_compact(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => format!("\"{}\"", s),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => serde_json::to_string(value)
            .unwrap_or_else(|_| value.to_string())
            .chars()
            .take(80)
            .collect::<String>()
            .pipe_truncate(80),
    }
}

trait PipeTruncate {
    fn pipe_truncate(self, max: usize) -> String;
}

impl PipeTruncate for String {
    fn pipe_truncate(self, max: usize) -> String {
        (self.len() > max)
            .then(|| format!("{}...", &self[..max.saturating_sub(3)]))
            .unwrap_or(self)
    }
}

/// Resolve a JSON Pointer (RFC 6901) against a JSON value.
fn resolve_json_pointer(value: &serde_json::Value, pointer: &str) -> Option<serde_json::Value> {
    (pointer == "/" || pointer.is_empty())
        .then(|| value.clone())
        .or_else(|| value.pointer(pointer).cloned())
}
