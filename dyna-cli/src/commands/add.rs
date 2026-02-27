//! `dyna add` command implementation.
//!
//! Supports:
//! - `dyna add <file.json>` — stage a single JSON file
//! - `dyna add <directory>` — recursively stage all `.json` files in a directory
//! - `dyna add <directory> --recursive` — explicit recursive flag (implied for dirs)

use anyhow::{Context, Result, bail};
use dyna_common::diff;
use dyna_common::models::StagedChange;
use std::path::{Path, PathBuf};

use crate::repository::Repository;

pub async fn execute(path: PathBuf) -> Result<()> {
    let repo = Repository::find_current()?;

    // Resolve the path relative to the working directory
    let abs_path = if path.is_absolute() {
        path.clone()
    } else {
        std::env::current_dir()?.join(&path)
    };

    if !abs_path.exists() {
        bail!("Path not found: {}", abs_path.display());
    }

    if abs_path.is_dir() {
        // Recursively add all .json files in the directory
        let json_files = collect_json_files_recursive(&abs_path, &repo.dyna_dir)?;

        if json_files.is_empty() {
            println!("No .json files found in {}", path.display());
            return Ok(());
        }

        println!(
            "Adding {} JSON file(s) from {}...\n",
            json_files.len(),
            path.display()
        );

        let mut staged_count = 0;
        let mut skipped_count = 0;
        let mut error_count = 0;

        for file_path in &json_files {
            match stage_single_file(&repo, file_path) {
                Ok(staged) => {
                    if staged {
                        // Compute the display path relative to the current directory
                        let display_path = file_path
                            .strip_prefix(&repo.work_dir)
                            .unwrap_or(file_path);
                        println!("  {} {}", "staged".green(), display_path.display());
                        staged_count += 1;
                    } else {
                        skipped_count += 1;
                    }
                }
                Err(e) => {
                    let display_path = file_path
                        .strip_prefix(&repo.work_dir)
                        .unwrap_or(file_path);
                    eprintln!(
                        "  {} {} — {}",
                        "error".red(),
                        display_path.display(),
                        e
                    );
                    error_count += 1;
                }
            }
        }

        println!();
        println!(
            "Summary: {} staged, {} unchanged, {} error(s)",
            staged_count, skipped_count, error_count
        );
    } else {
        // Single file mode
        stage_single_file_verbose(&repo, &abs_path, &path)?;
    }

    Ok(())
}

/// Stage a single file and print verbose output. Used for the single-file case.
fn stage_single_file_verbose(repo: &Repository, abs_path: &Path, display_path: &Path) -> Result<()> {
    // Read and validate JSON
    let content = std::fs::read_to_string(abs_path).context("Failed to read file")?;
    let current: serde_json::Value =
        serde_json::from_str(&content).context("File is not valid JSON")?;

    // Derive resource ID from filename
    let resource_id = Repository::resource_id_from_path(abs_path);

    // Load the previous snapshot (if any)
    let previous = repo.load_snapshot(&resource_id)?;

    // Compute diff operations
    let operations = match &previous {
        Some(prev) => diff::diff(prev, &current),
        None => vec![dyna_common::models::PatchOperation::Add {
            path: "/".to_string(),
            value: current.clone(),
        }],
    };

    if operations.is_empty() {
        println!("No changes detected in {}", display_path.display());
        return Ok(());
    }

    // Create the staged change
    let relative_path = display_path.display().to_string();
    let staged = StagedChange {
        resource_id: resource_id.clone(),
        file_path: relative_path.clone(),
        previous,
        current,
        operations,
    };

    repo.stage_change(&staged)?;

    println!("Staged: {} (resource: {})", relative_path, resource_id);
    println!("  {} operation(s) detected", staged.operations.len());

    Ok(())
}

/// Stage a single file. Returns `true` if the file was staged (had changes),
/// `false` if it was skipped (no changes).
fn stage_single_file(repo: &Repository, abs_path: &Path) -> Result<bool> {
    let content = std::fs::read_to_string(abs_path).context("Failed to read file")?;
    let current: serde_json::Value =
        serde_json::from_str(&content).context("File is not valid JSON")?;

    let resource_id = Repository::resource_id_from_path(abs_path);
    let previous = repo.load_snapshot(&resource_id)?;

    let operations = match &previous {
        Some(prev) => diff::diff(prev, &current),
        None => vec![dyna_common::models::PatchOperation::Add {
            path: "/".to_string(),
            value: current.clone(),
        }],
    };

    if operations.is_empty() {
        return Ok(false);
    }

    let relative_path = abs_path
        .strip_prefix(&repo.work_dir)
        .unwrap_or(abs_path)
        .display()
        .to_string();

    let staged = StagedChange {
        resource_id,
        file_path: relative_path,
        previous,
        current,
        operations,
    };

    repo.stage_change(&staged)?;
    Ok(true)
}

/// Recursively collect all `.json` files in a directory, skipping the `.dyna/`
/// metadata directory and hidden directories.
fn collect_json_files_recursive(dir: &Path, dyna_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    walk_dir(dir, dyna_dir, &mut results)?;
    results.sort();
    Ok(results)
}

fn walk_dir(dir: &Path, dyna_dir: &Path, results: &mut Vec<PathBuf>) -> Result<()> {
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

        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            // Skip hidden directories
            if name.starts_with('.') {
                continue;
            }
            walk_dir(&path, dyna_dir, results)?;
        } else if path.extension().map_or(false, |ext| ext == "json") {
            results.push(path);
        }
    }

    Ok(())
}

// Bring colored trait into scope for the green/red methods on &str
use colored::Colorize;
