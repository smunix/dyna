//! `dyna add` command implementation.
//!
//! Supports:
//! - `dyna add <file.json>` — stage a single JSON file
//! - `dyna add <directory>` — recursively stage all `.json` files in a directory
//! - `dyna add <directory> --recursive` — explicit recursive flag (implied for dirs)
//! - `dyna add --delete <file.json>` — stage the removal of a single tracked file
//! - `dyna add --delete <directory>` — stage the removal of all deleted tracked
//!   files whose paths fall under the given directory
//! - `dyna add --delete "glob/pattern"` — stage the removal of all deleted tracked
//!   files matching a glob pattern (e.g. `"data/**/*.json"`, `"users/*.json"`)

use anyhow::{Context, Result, bail};
use colored::Colorize;
use dyna_core::diff;
use dyna_core::models::{PatchOperation, StagedChange};
use itertools::{izip, Itertools};
use std::path::{Path, PathBuf};

use crate::repository::Repository;

/// Detect whether a string contains glob meta-characters (`*`, `?`, `[`, `{`).
fn is_glob_pattern(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{')
}

pub async fn execute(pattern: String, delete: bool) -> Result<()> {
    let repo = Repository::find_current()?;

    // --delete mode: stage the removal of files that no longer exist on disk.
    // Supports single files, directories, and glob patterns.
    if delete {
        return execute_delete(&repo, &pattern);
    }

    // Normal (non-delete) mode: stage additions/modifications.
    let path = PathBuf::from(&pattern);
    let abs_path = path
        .is_absolute()
        .then(|| path.clone())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join(&path));

    if !abs_path.exists() {
        // If the file doesn't exist, check whether it's a tracked resource that
        // was deleted — suggest using --delete.
        let resource_id = repo.resource_id_from_path(&abs_path);
        repo.load_snapshot(&resource_id)?
            .map(|_| {
                bail!(
                    "File '{}' has been deleted from disk.\n\
                     Use 'dyna add --delete {}' to stage the removal.",
                    pattern,
                    pattern
                )
            })
            .unwrap_or_else(|| bail!("Path not found: {}", abs_path.display()))
    } else if abs_path.is_dir() {
        let json_files = collect_json_files_recursive(&abs_path, &repo.dyna_dir)?;

        if json_files.is_empty() {
            println!("No .json files found in {}", pattern);
            return Ok(());
        }

        println!(
            "Adding {} JSON file(s) from {}...\n",
            json_files.len(),
            pattern
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
        stage_single_file_verbose(&repo, &abs_path, &PathBuf::from(&pattern))
    }
}

// ---------------------------------------------------------------------------
// Delete mode: single file, directory, or glob
// ---------------------------------------------------------------------------

/// Entry point for `--delete` mode. Dispatches to the appropriate handler
/// based on whether the pattern is a glob, a directory path, or a single file.
fn execute_delete(repo: &Repository, pattern: &str) -> Result<()> {
    if is_glob_pattern(pattern) {
        stage_deletions_by_glob(repo, pattern)
    } else {
        let path = PathBuf::from(pattern);
        let abs_path = path
            .is_absolute()
            .then(|| path.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join(&path));

        // If the path points to a directory that still exists, or if the path
        // *would be* a directory (ends with '/' or matches a known directory
        // prefix in snapshots), treat it as a directory deletion.
        if abs_path.is_dir() || pattern.ends_with('/') {
            stage_deletions_in_directory(repo, &abs_path, pattern)
        } else {
            // Check if this path is a prefix of any snapshot resource IDs,
            // which would indicate it was a directory that has been fully deleted.
            let resource_prefix = repo.resource_id_from_path(&abs_path);
            let snapshots = repo.load_all_snapshots()?;
            let has_children = izip!(snapshots.keys())
                .any(|rid| rid.starts_with(&format!("{}.", resource_prefix)) || *rid == resource_prefix);

            // If the path doesn't exist on disk and has snapshot children, treat
            // it as a deleted directory.
            let is_deleted_dir = !abs_path.exists()
                && !snapshots.contains_key(&resource_prefix)
                && has_children;

            if is_deleted_dir {
                stage_deletions_in_directory(repo, &abs_path, pattern)
            } else {
                stage_single_deletion(repo, &abs_path, &path)
            }
        }
    }
}

/// Stage the removal of a single tracked file that has been deleted from the
/// filesystem. Creates a `StagedChange` with a `null` current value and a
/// single `Remove` operation at the root path.
fn stage_single_deletion(repo: &Repository, abs_path: &Path, display_path: &Path) -> Result<()> {
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
        operations: vec![PatchOperation::Remove {
            path: "/".to_string(),
        }],
    };

    repo.stage_change(&staged)?;

    println!(
        "Staged deletion: {} (resource: {})",
        relative_path.red(),
        resource_id
    );

    Ok(())
}

/// Stage the removal of all deleted tracked files whose resource IDs fall
/// under the given directory path. Walks all snapshots and filters those
/// whose reconstructed file path starts with the directory prefix.
fn stage_deletions_in_directory(repo: &Repository, abs_dir: &Path, display_dir: &str) -> Result<()> {
    let snapshots = repo.load_all_snapshots()?;

    // Compute the resource ID prefix for the directory. For a directory path
    // like `<work_dir>/data/users`, the prefix is `data.users`.
    let dir_resource_prefix = repo.resource_id_from_path(abs_dir);

    println!(
        "Scanning for deleted tracked files under {}...\n",
        display_dir
    );

    // Find all snapshot resource IDs that fall under this directory and whose
    // file no longer exists on disk.
    let (staged_count, skipped_count, error_count) = izip!(snapshots.keys().sorted())
        .filter(|resource_id| {
            resource_id.starts_with(&dir_resource_prefix)
                && (resource_id.len() == dir_resource_prefix.len()
                    || resource_id[dir_resource_prefix.len()..].starts_with('.'))
        })
        .fold(
            (0usize, 0usize, 0usize),
            |(staged, skipped, errors), resource_id| {
                let file_path = repo.path_for_resource_id(resource_id);
                if file_path.exists() {
                    // File still exists — skip (not deleted)
                    (staged, skipped + 1, errors)
                } else {
                    match stage_deletion_by_resource_id(repo, resource_id, snapshots.get(resource_id).unwrap()) {
                        Ok(()) => {
                            let relative = file_path
                                .strip_prefix(&repo.work_dir)
                                .unwrap_or(&file_path)
                                .display()
                                .to_string();
                            println!("  {} {}", "staged".green(), relative.red());
                            (staged + 1, skipped, errors)
                        }
                        Err(e) => {
                            eprintln!("  {} {} — {}", "error".red(), resource_id, e);
                            (staged, skipped, errors + 1)
                        }
                    }
                }
            },
        );

    (staged_count == 0 && error_count == 0)
        .then(|| {
            println!(
                "No deleted tracked files found under {}",
                display_dir.dimmed()
            );
        })
        .unwrap_or_else(|| {
            println!(
                "\nSummary: {} deletion(s) staged, {} still present, {} error(s)",
                staged_count, skipped_count, error_count
            );
        });

    Ok(())
}

/// Stage the removal of all deleted tracked files matching a glob pattern.
/// The glob is resolved relative to the working directory. Snapshot resource
/// IDs are converted back to file paths for matching.
fn stage_deletions_by_glob(repo: &Repository, pattern: &str) -> Result<()> {
    let snapshots = repo.load_all_snapshots()?;

    // Resolve the glob pattern relative to the working directory
    let abs_pattern = PathBuf::from(pattern)
        .is_absolute()
        .then(|| pattern.to_string())
        .unwrap_or_else(|| {
            repo.work_dir
                .join(pattern)
                .display()
                .to_string()
        });

    let glob_pattern = glob::Pattern::new(&abs_pattern)
        .map_err(|e| anyhow::anyhow!("Invalid glob pattern '{}': {}", pattern, e))?;

    println!(
        "Scanning for deleted tracked files matching '{}'...\n",
        pattern
    );

    // Walk all snapshots, convert resource IDs to file paths, and match
    // against the glob pattern.
    let (staged_count, skipped_count, error_count) = izip!(snapshots.keys().sorted())
        .map(|resource_id| {
            let file_path = repo.path_for_resource_id(resource_id);
            (resource_id, file_path)
        })
        .filter(|(_, file_path)| glob_pattern.matches_path(file_path))
        .fold(
            (0usize, 0usize, 0usize),
            |(staged, skipped, errors), (resource_id, file_path)| {
                if file_path.exists() {
                    // File still exists — skip
                    (staged, skipped + 1, errors)
                } else {
                    match stage_deletion_by_resource_id(repo, resource_id, snapshots.get(resource_id).unwrap()) {
                        Ok(()) => {
                            let relative = file_path
                                .strip_prefix(&repo.work_dir)
                                .unwrap_or(&file_path)
                                .display()
                                .to_string();
                            println!("  {} {}", "staged".green(), relative.red());
                            (staged + 1, skipped, errors)
                        }
                        Err(e) => {
                            eprintln!("  {} {} — {}", "error".red(), resource_id, e);
                            (staged, skipped, errors + 1)
                        }
                    }
                }
            },
        );

    (staged_count == 0 && error_count == 0)
        .then(|| {
            println!(
                "No deleted tracked files matching '{}'",
                pattern.dimmed()
            );
        })
        .unwrap_or_else(|| {
            println!(
                "\nSummary: {} deletion(s) staged, {} still present, {} error(s)",
                staged_count, skipped_count, error_count
            );
        });

    Ok(())
}

/// Stage a deletion for a resource given its resource_id and snapshot value.
/// Used by the directory and glob deletion handlers.
fn stage_deletion_by_resource_id(
    repo: &Repository,
    resource_id: &str,
    previous: &serde_json::Value,
) -> Result<()> {
    let file_path = repo.path_for_resource_id(resource_id);
    let relative_path = file_path
        .strip_prefix(&repo.work_dir)
        .unwrap_or(&file_path)
        .display()
        .to_string();

    let staged = StagedChange {
        resource_id: resource_id.to_string(),
        file_path: relative_path,
        previous: Some(previous.clone()),
        current: serde_json::Value::Null,
        operations: vec![PatchOperation::Remove {
            path: "/".to_string(),
        }],
    };

    repo.stage_change(&staged)
}

// ---------------------------------------------------------------------------
// Normal (non-delete) staging helpers
// ---------------------------------------------------------------------------

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
