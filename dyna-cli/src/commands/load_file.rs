//! `dyna load-file` command implementation.
//!
//! Loads a JSON file containing an array of resource objects. Each object must
//! have a `res_id` field that specifies the resource ID; the remaining fields
//! form the resource content. Each matching resource is written to the working
//! directory and staged for the next commit.
//!
//! ## Options
//!
//! - `--limit <N>`: Load at most N resources from the file.
//! - `--filter <regex>`: Only load resources whose `res_id` matches the regex.
//!
//! ## Example input file
//!
//! ```json
//! [
//!   { "res_id": "acme.entity.User", "name": "Alice", "role": "admin" },
//!   { "res_id": "acme.entity.Role", "name": "admin", "permissions": ["read", "write"] }
//! ]
//! ```

use anyhow::{Context, Result};
use colored::Colorize;
use dyna_core::diff;
use dyna_core::models::{PatchOperation, StagedChange};
use regex::Regex;
use serde_json::Value;
use std::path::PathBuf;

use crate::repository::Repository;

pub async fn execute(
    file_path: PathBuf,
    limit: Option<usize>,
    filter: Option<String>,
) -> Result<()> {
    let repo = Repository::find_current()?;

    // Read and parse the JSON file
    let content = std::fs::read_to_string(&file_path)
        .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

    let resources: Vec<Value> = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse JSON array from: {}", file_path.display()))?;

    if resources.is_empty() {
        println!("No resources found in {}", file_path.display());
        return Ok(());
    }

    // Compile the regex filter if provided
    let regex_filter = filter
        .as_deref()
        .map(|pattern| {
            Regex::new(pattern)
                .with_context(|| format!("Invalid regex pattern: {}", pattern))
        })
        .transpose()?;

    println!(
        "Loading resources from {}...",
        file_path.display().to_string().cyan()
    );

    if let Some(ref re) = regex_filter {
        println!("  Filter: {}", re.as_str().yellow());
    }
    if let Some(n) = limit {
        println!("  Limit:  {}", n.to_string().yellow());
    }
    println!();

    // Process resources: extract res_id, filter, limit, then stage
    let (staged_count, skipped_count, error_count) = resources
        .into_iter()
        .enumerate()
        // Extract res_id and build the resource content (without res_id field)
        .filter_map(|(idx, mut obj)| {
            let res_id = obj
                .as_object()
                .and_then(|o| o.get("res_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            match res_id {
                Some(id) => {
                    // Remove the res_id field from the content
                    obj.as_object_mut().map(|o| o.remove("res_id"));
                    Some((idx, id, obj))
                }
                None => {
                    eprintln!(
                        "  {} entry #{}: missing or non-string 'res_id' field",
                        "skip".yellow(),
                        idx
                    );
                    None
                }
            }
        })
        // Apply regex filter
        .filter(|(_, res_id, _)| {
            regex_filter
                .as_ref()
                .map(|re| re.is_match(res_id))
                .unwrap_or(true)
        })
        // Apply limit
        .take(limit.unwrap_or(usize::MAX))
        // Stage each resource
        .fold(
            (0usize, 0usize, 0usize),
            |(staged, skipped, errors), (_idx, res_id, content)| {
                match stage_resource(&repo, &res_id, &content) {
                    Ok(true) => {
                        println!("  {} {}", "staged".green(), res_id);
                        (staged + 1, skipped, errors)
                    }
                    Ok(false) => {
                        println!("  {} {} (unchanged)", "skip".dimmed(), res_id.dimmed());
                        (staged, skipped + 1, errors)
                    }
                    Err(e) => {
                        eprintln!("  {} {} — {}", "error".red(), res_id, e);
                        (staged, skipped, errors + 1)
                    }
                }
            },
        );

    println!();
    println!(
        "Summary: {} staged, {} unchanged, {} error(s)",
        staged_count.to_string().green(),
        skipped_count,
        error_count
    );

    if staged_count > 0 {
        println!(
            "\nUse '{}' to record these changes.",
            "dyna commit -m \"<message>\"".cyan()
        );
    }

    Ok(())
}

/// Write a resource to the working directory and stage it.
///
/// Returns `Ok(true)` if the resource was staged (new or modified),
/// `Ok(false)` if the resource is unchanged from its snapshot.
fn stage_resource(repo: &Repository, resource_id: &str, content: &Value) -> Result<bool> {
    let relative_path = repo.relative_path_for_resource_id(resource_id);

    // Pretty-print the JSON content for the working file
    let json_str = serde_json::to_string_pretty(content)
        .context("Failed to serialize resource content")?;

    // Load the existing snapshot (if any) for diff computation
    let previous = repo.load_snapshot(resource_id)?;

    // Check if the content is unchanged from the snapshot
    if let Some(ref prev) = previous {
        if prev == content {
            return Ok(false);
        }
    }

    // Write the resource file to the working directory
    repo.write_work_file(&relative_path, &json_str)?;

    // Compute the diff operations
    let operations = previous
        .as_ref()
        .map(|prev| diff::diff(prev, content))
        .unwrap_or_else(|| {
            vec![PatchOperation::Add {
                path: "/".to_string(),
                value: content.clone(),
            }]
        });

    // Create and store the staged change
    let staged = StagedChange {
        resource_id: resource_id.to_string(),
        file_path: relative_path,
        previous,
        current: content.clone(),
        operations,
    };

    repo.stage_change(&staged)?;

    Ok(true)
}
