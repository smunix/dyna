//! `dyna add` command implementation.
//!
//! Supports:
//! - `dyna add <file.json>` — stage a single JSON file
//! - `dyna add <directory>` — recursively stage all `.json` files in a directory
//! - `dyna add <directory> --recursive` — explicit recursive flag (implied for dirs)
//! - `dyna add --delete <file.json>` — stage the removal of a tracked file that
//!   has been deleted from the filesystem

use anyhow::{Context, Result, bail};
use colored::Colorize;
use dyna_core::diff;
use dyna_core::models::{PatchOperation, StagedChange};
use itertools::{izip, Itertools};
use std::path::{Path, PathBuf};

use crate::repository::Repository;

pub async fn execute(path: PathBuf, delete: bool) -> Result<()> {
    let repo = Repository::find_current()?;

    let abs_path = path
        .is_absolute()
        .then(|| path.clone())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join(&path));

    // --delete mode: stage the removal of a file that no longer exists on disk
    if delete {
        return stage_deletion(&repo, &abs_path, &path);
    }

    if !abs_path.exists() {
        // If the file doesn't exist, check whether it's a tracked resource that
        // was deleted — suggest using --delete.
        let resource_id = repo.resource_id_from_path(&abs_path);
        repo.load_snapshot(&resource_id)?
            .map(|_| {
                bail!(
                    "File '{}' has been deleted from disk.\n\
                     Use 'dyna add --delete {}' to stage the removal.",
                    path.display(),
                    path.display()
                )
            })
            .unwrap_or_else(|| bail!("Path not found: {}", abs_path.display()))
    } else if abs_path.is_dir() {
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

        // Process all files via fold, accumulating (staged, skipped, errors) counts
        let (staged_count, skipped_count, error_count) = izip!(&json_files)
            .fold((0usize, 0usize, 0usize), |(staged, skipped, errors), file_path| {
                let display_path = file_path
                    .strip_prefix(&repo.work_dir)
                    .unwrap_or(file_path);
                match stage_single_file(&repo, file_path) {
                    Ok(true) => {
                        println!("  {} {}", "staged".green(), display_path.display());
                        (staged + 1, skipped, errors)
                    }
                    Ok(false) => (staged, skipped + 1, errors),
                    Err(e) => {
                        eprintln!("  {} {} — {}", "error".red(), display_path.display(), e);
                        (staged, skipped, errors + 1)
                    }
                }
            });

        println!();
        println!(
            "Summary: {} staged, {} unchanged, {} error(s)",
            staged_count, skipped_count, error_count
        );
        Ok(())
    } else {
        stage_single_file_verbose(&repo, &abs_path, &path)
    }
}

/// Stage the removal of a tracked file that has been deleted from the
/// filesystem. Creates a `StagedChange` with a `null` current value and a
/// single `Remove` operation at the root path.
fn stage_deletion(repo: &Repository, abs_path: &Path, display_path: &Path) -> Result<()> {
    let resource_id = repo.resource_id_from_path(abs_path);

    let previous = repo
        .load_snapshot(&resource_id)?
        .ok_or_else(|| anyhow::anyhow!(
            "Cannot stage deletion: '{}' is not tracked (no snapshot found for resource '{}')",
            display_path.display(),
            resource_id
        ))?;

    // If the file still exists on disk, warn the user
    if abs_path.exists() {
        bail!(
            "File '{}' still exists on disk. Delete it first, or use 'dyna add {}' to stage modifications.",
            display_path.display(),
            display_path.display()
        );
    }

    let operations = vec![PatchOperation::Remove {
        path: "/".to_string(),
    }];

    let relative_path = abs_path
        .strip_prefix(&repo.work_dir)
        .unwrap_or(abs_path)
        .display()
        .to_string();

    let staged = StagedChange {
        resource_id: resource_id.clone(),
        file_path: relative_path.clone(),
        previous: Some(previous),
        current: serde_json::Value::Null,
        operations,
    };

    repo.stage_change(&staged)?;

    println!(
        "Staged deletion: {} (resource: {})",
        relative_path.red(),
        resource_id
    );

    Ok(())
}

/// Stage a single file and print verbose output. Used for the single-file case.
fn stage_single_file_verbose(repo: &Repository, abs_path: &Path, display_path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(abs_path).context("Failed to read file")?;
    let current: serde_json::Value =
        serde_json::from_str(&content).context("File is not valid JSON")?;

    let resource_id = repo.resource_id_from_path(abs_path);
    let previous = repo.load_snapshot(&resource_id)?;

    let operations = previous
        .as_ref()
        .map(|prev| diff::diff(prev, &current))
        .unwrap_or_else(|| {
            vec![PatchOperation::Add {
                path: "/".to_string(),
                value: current.clone(),
            }]
        });

    if operations.is_empty() {
        println!("No changes detected in {}", display_path.display());
        return Ok(());
    }

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

    let resource_id = repo.resource_id_from_path(abs_path);
    let previous = repo.load_snapshot(&resource_id)?;

    let operations = previous
        .as_ref()
        .map(|prev| diff::diff(prev, &current))
        .unwrap_or_else(|| {
            vec![PatchOperation::Add {
                path: "/".to_string(),
                value: current.clone(),
            }]
        });

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

    repo.stage_change(&staged).map(|()| true)
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
                    .map(|_| walk_dir(&path, dyna_dir, results))
                    .unwrap_or(Ok(()))
            } else {
                let is_json = path.extension().map_or(false, |ext| ext == "json");
                if is_json {
                    results.push(path);
                }
                Ok(())
            }
        })
}
