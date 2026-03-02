//! # dyna-wasm
//!
//! WebAssembly client for the Dyna distributed CRUD system.
//!
//! This crate compiles to WASM and provides a JavaScript-friendly API for
//! interacting with the Dyna remote server directly from the browser. All
//! local repository state is stored in an in-memory VFS (`MemoryFS`), so
//! the entire `.dyna/` tree lives in browser memory.
//!
//! ## Usage from JavaScript
//!
//! ```js
//! import init, { DynaClient } from './dyna_wasm.js';
//!
//! await init();
//! const client = new DynaClient();
//! client.init_repo("https://my-dyna-server.example.com");
//!
//! // Write a JSON file and stage it
//! client.write_file("data/users/config.json", JSON.stringify({ admin: true }));
//! client.add("data/users/config.json");
//! client.commit("Initial config");
//!
//! // Push to remote
//! await client.push();
//!
//! // Get status
//! const status = client.status();
//! console.log(JSON.parse(status));
//! ```

mod repository;
mod sync_client;

use anyhow::Result;
use dyna_core::diff;
use dyna_core::models::*;
use dyna_core::patch;
use dyna_core::protocol::*;
use itertools::izip;
use repository::Repository;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sync_client::SyncClient;
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// JS-friendly result types (serialised as JSON)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct StatusResult {
    channel: String,
    staged: Vec<StagedFileInfo>,
    modified: Vec<String>,
    deleted: Vec<String>,
    unstaged_on_staged: Vec<String>,
    conflicts: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct StagedFileInfo {
    resource_id: String,
    ops: usize,
    kind: String, // "new", "modified", "deleted"
}

#[derive(Serialize, Deserialize)]
struct ChannelInfo {
    name: String,
    changeset_count: usize,
    head: Option<String>,
    is_current: bool,
}

#[derive(Serialize, Deserialize)]
struct LogEntry {
    change_id: String,
    commit_hash: String,
    message: String,
    author: String,
    created_at: String,
    patch_count: usize,
    immutable: bool,
}

// ---------------------------------------------------------------------------
// DynaClient — the main WASM-exported class
// ---------------------------------------------------------------------------

/// The main entry point for browser-based Dyna operations.
///
/// Wraps an in-memory `Repository` and a `SyncClient` for remote operations.
/// All methods that return data use JSON strings for JS interop.
#[wasm_bindgen]
pub struct DynaClient {
    repo: Repository,
    remote_url: Option<String>,
}

#[wasm_bindgen]
impl DynaClient {
    // -----------------------------------------------------------------------
    // Constructor & Initialisation
    // -----------------------------------------------------------------------

    /// Create a new DynaClient with a fresh in-memory repository.
    #[wasm_bindgen(constructor)]
    pub fn new() -> DynaClient {
        // Set up console_error_panic_hook for better error messages in the browser
        console_error_panic_hook::set_once();
        DynaClient {
            repo: Repository::new(),
            remote_url: None,
        }
    }

    /// Initialise the in-memory repository and set the remote URL.
    #[wasm_bindgen]
    pub fn init_repo(&mut self, remote_url: &str) -> Result<(), JsError> {
        self.repo.init().map_err(to_js_error)?;
        self.remote_url = Some(remote_url.trim_end_matches('/').to_string());

        let mut config = self.repo.load_config().map_err(to_js_error)?;
        config.remote_url = self.remote_url.clone();
        self.repo.save_config(&config).map_err(to_js_error)?;

        Ok(())
    }

    /// Check whether the repository has been initialised.
    #[wasm_bindgen]
    pub fn is_initialized(&self) -> bool {
        self.repo.is_initialized()
    }

    /// Set the user name and email for commit authorship.
    #[wasm_bindgen]
    pub fn set_user(&mut self, name: &str, email: &str) -> Result<(), JsError> {
        let mut config = self.repo.load_config().map_err(to_js_error)?;
        config.user.name = name.to_string();
        config.user.email = email.to_string();
        self.repo.save_config(&config).map_err(to_js_error)
    }

    /// Set or update the remote URL.
    #[wasm_bindgen]
    pub fn set_remote(&mut self, url: &str) -> Result<(), JsError> {
        self.remote_url = Some(url.trim_end_matches('/').to_string());
        let mut config = self.repo.load_config().map_err(to_js_error)?;
        config.remote_url = self.remote_url.clone();
        self.repo.save_config(&config).map_err(to_js_error)
    }

    // -----------------------------------------------------------------------
    // File I/O (working directory)
    // -----------------------------------------------------------------------

    /// Write a file to the in-memory working directory.
    #[wasm_bindgen]
    pub fn write_file(&self, path: &str, content: &str) -> Result<(), JsError> {
        self.repo.write_work_file(path, content).map_err(to_js_error)
    }

    /// Read a file from the in-memory working directory.
    #[wasm_bindgen]
    pub fn read_file(&self, path: &str) -> Result<String, JsError> {
        self.repo.read_work_file(path).map_err(to_js_error)
    }

    /// Check if a file exists in the working directory.
    #[wasm_bindgen]
    pub fn file_exists(&self, path: &str) -> Result<bool, JsError> {
        self.repo.work_file_exists(path).map_err(to_js_error)
    }

    /// Delete a file from the working directory.
    #[wasm_bindgen]
    pub fn delete_file(&self, path: &str) -> Result<(), JsError> {
        self.repo.remove_work_file(path).map_err(to_js_error)
    }

    /// List all JSON files in the working directory.
    /// Returns a JSON array of relative paths.
    #[wasm_bindgen]
    pub fn list_files(&self) -> Result<String, JsError> {
        self.repo
            .list_work_json_files()
            .map_err(to_js_error)
            .and_then(|files| serde_json::to_string(&files).map_err(|e| JsError::new(&e.to_string())))
    }

    // -----------------------------------------------------------------------
    // Status
    // -----------------------------------------------------------------------

    /// Get repository status as a JSON string.
    #[wasm_bindgen]
    pub fn status(&self) -> Result<String, JsError> {
        let channel = self.repo.current_channel_name().map_err(to_js_error)?;
        let staged_changes = self.repo.load_staged_changes().map_err(to_js_error)?;
        let snapshots = self.repo.load_all_snapshots().map_err(to_js_error)?;
        let json_files = self.repo.list_work_json_files().map_err(to_js_error)?;
        let conflicts = self.repo.list_conflicted_resources().map_err(to_js_error)?;

        let staged_ids: std::collections::HashSet<String> = izip!(&staged_changes)
            .map(|s| s.resource_id.clone())
            .collect();

        // Staged files info
        let staged: Vec<StagedFileInfo> = izip!(&staged_changes)
            .map(|s| {
                let kind = if s.previous.is_none() {
                    "new"
                } else if s.current.is_null() {
                    "deleted"
                } else {
                    "modified"
                };
                StagedFileInfo {
                    resource_id: s.resource_id.clone(),
                    ops: s.operations.len(),
                    kind: kind.to_string(),
                }
            })
            .collect();

        // Modified but not staged
        let modified: Vec<String> = izip!(&json_files)
            .filter_map(|rel| {
                let resource_id = self.repo.resource_id_from_relative(rel);
                if staged_ids.contains(&resource_id) {
                    return None;
                }
                snapshots.get(&resource_id).and_then(|snapshot| {
                    self.repo
                        .read_work_file(rel)
                        .ok()
                        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
                        .and_then(|current| (current != *snapshot).then_some(resource_id))
                })
            })
            .collect();

        // Deleted tracked files
        let fs_ids: std::collections::HashSet<String> = izip!(&json_files)
            .map(|rel| self.repo.resource_id_from_relative(rel))
            .collect();
        let deleted: Vec<String> = izip!(snapshots.keys())
            .filter(|id| !fs_ids.contains(*id) && !staged_ids.contains(*id))
            .cloned()
            .collect();

        // Unstaged modifications on staged files
        let unstaged_on_staged: Vec<String> = izip!(&staged_changes)
            .filter(|s| !s.current.is_null())
            .filter_map(|s| {
                let rel = self.repo.relative_path_for_resource_id(&s.resource_id);
                self.repo
                    .read_work_file(&rel)
                    .ok()
                    .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
                    .and_then(|disk_val| (disk_val != s.current).then_some(s.resource_id.clone()))
            })
            .collect();

        let result = StatusResult {
            channel,
            staged,
            modified,
            deleted,
            unstaged_on_staged,
            conflicts,
        };

        serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
    }

    // -----------------------------------------------------------------------
    // Add (stage)
    // -----------------------------------------------------------------------

    /// Stage a file for commit. The path is relative to the working directory.
    #[wasm_bindgen]
    pub fn add(&self, path: &str) -> Result<(), JsError> {
        let resource_id = self.repo.resource_id_from_relative(path);
        let content = self.repo.read_work_file(path).map_err(to_js_error)?;
        let current: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| JsError::new(&e.to_string()))?;

        let previous = self.repo.load_snapshot(&resource_id).map_err(to_js_error)?;
        let operations = diff::diff(
            previous.as_ref().unwrap_or(&serde_json::json!({})),
            &current,
        );

        let staged = StagedChange {
            resource_id,
            file_path: path.to_string(),
            previous,
            current,
            operations,
        };
        self.repo.stage_change(&staged).map_err(to_js_error)
    }

    /// Stage a file deletion.
    #[wasm_bindgen]
    pub fn add_delete(&self, path: &str) -> Result<(), JsError> {
        let resource_id = self.repo.resource_id_from_relative(path);
        let previous = self
            .repo
            .load_snapshot(&resource_id)
            .map_err(to_js_error)?
            .ok_or_else(|| JsError::new(&format!("No snapshot found for '{}'", resource_id)))?;

        let staged = StagedChange {
            resource_id,
            file_path: path.to_string(),
            previous: Some(previous),
            current: serde_json::Value::Null,
            operations: vec![PatchOperation::Remove {
                path: "/".to_string(),
            }],
        };
        self.repo.stage_change(&staged).map_err(to_js_error)
    }

    // -----------------------------------------------------------------------
    // Commit
    // -----------------------------------------------------------------------

    /// Commit all staged changes with the given message.
    /// Returns the new changeset's change_id.
    #[wasm_bindgen]
    pub fn commit(&self, message: &str) -> Result<String, JsError> {
        let staged_changes = self.repo.load_staged_changes().map_err(to_js_error)?;
        if staged_changes.is_empty() {
            return Err(JsError::new("Nothing to commit: no staged changes."));
        }

        let config = self.repo.load_config().map_err(to_js_error)?;
        let author = config.user.name.clone();
        let channel_name = self.repo.current_channel_name().map_err(to_js_error)?;

        // Protect the main channel from direct commits
        if channel_name == "main" {
            return Err(JsError::new(
                "Cannot commit directly to the 'main' channel. \
                 Switch to a feature channel first, then promote to main.",
            ));
        }

        let parent = self.repo.working_change_id().map_err(to_js_error)?;
        let parents = parent.into_iter().collect::<Vec<_>>();

        // Build patches from staged changes
        let patches: Vec<Patch> = izip!(&staged_changes)
            .map(|s| patch::build_patch(s))
            .collect();

        let changeset = Changeset::new(author, message.to_string(), parents, patches);

        // Save changeset
        self.repo.save_changeset(&changeset).map_err(to_js_error)?;

        // Update channel
        let mut channel = self.repo.load_channel(&channel_name).map_err(to_js_error)?;
        channel.append_changeset(changeset.change_id.clone());
        self.repo.save_channel(&channel).map_err(to_js_error)?;

        // Update working change
        self.repo
            .set_working_change(Some(&changeset.change_id))
            .map_err(to_js_error)?;

        // Update snapshots: save current for modifications, remove for deletions
        izip!(&staged_changes).try_for_each(|s| {
            if s.current.is_null() {
                self.repo.remove_snapshot(&s.resource_id)
            } else {
                self.repo.save_snapshot(&s.resource_id, &s.current)
            }
        }).map_err(to_js_error)?;

        // Clear staging
        self.repo.clear_staging().map_err(to_js_error)?;

        Ok(changeset.change_id)
    }

    // -----------------------------------------------------------------------
    // Log
    // -----------------------------------------------------------------------

    /// Get the changeset log for the current channel as a JSON array.
    #[wasm_bindgen]
    pub fn log(&self) -> Result<String, JsError> {
        let channel_name = self.repo.current_channel_name().map_err(to_js_error)?;
        let channel = self.repo.load_channel(&channel_name).map_err(to_js_error)?;

        let entries: Vec<LogEntry> = izip!(&channel.changesets)
            .rev()
            .filter_map(|cid| {
                self.repo.load_changeset(cid).ok().map(|cs| LogEntry {
                    change_id: cs.change_id,
                    commit_hash: cs.commit_hash,
                    message: cs.message,
                    author: cs.author,
                    created_at: cs.created_at.to_rfc3339(),
                    patch_count: cs.patches.len(),
                    immutable: cs.immutable,
                })
            })
            .collect();

        serde_json::to_string(&entries).map_err(|e| JsError::new(&e.to_string()))
    }

    // -----------------------------------------------------------------------
    // Channel management
    // -----------------------------------------------------------------------

    /// List all local channels. Returns JSON array.
    #[wasm_bindgen]
    pub fn list_channels(&self) -> Result<String, JsError> {
        let current = self.repo.current_channel_name().map_err(to_js_error)?;
        let channels = self.repo.list_channels().map_err(to_js_error)?;

        let infos: Vec<ChannelInfo> = izip!(&channels)
            .map(|ch| ChannelInfo {
                name: ch.name.clone(),
                changeset_count: ch.changesets.len(),
                head: ch.head_change_id.clone(),
                is_current: ch.name == current,
            })
            .collect();

        serde_json::to_string(&infos).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Create a new channel, optionally forking from an existing one.
    #[wasm_bindgen]
    pub fn create_channel(&self, name: &str, fork_from: Option<String>) -> Result<(), JsError> {
        self.repo
            .create_channel(name, fork_from.as_deref())
            .map_err(to_js_error)
    }

    /// Switch to a different channel.
    /// Cleans the working directory and restores files from the target channel.
    #[wasm_bindgen]
    pub fn switch_channel(&self, name: &str) -> Result<(), JsError> {
        let staged = self.repo.load_staged_changes().map_err(to_js_error)?;
        if !staged.is_empty() {
            return Err(JsError::new(
                "Cannot switch channels: you have staged but uncommitted changes.",
            ));
        }

        let target_channel = self.repo.load_channel(name).map_err(to_js_error)?;

        // Clean working directory: remove files tracked by current channel's snapshots
        let snapshots = self.repo.load_all_snapshots().map_err(to_js_error)?;
        izip!(snapshots.keys()).try_for_each(|resource_id| {
            let rel = self.repo.relative_path_for_resource_id(resource_id);
            self.repo.remove_work_file(&rel)
        }).map_err(to_js_error)?;

        // Switch HEAD first so snapshot methods resolve to the target channel
        self.repo.set_current_channel(name).map_err(to_js_error)?;
        self.repo
            .set_working_change(target_channel.head_change_id.as_deref())
            .map_err(to_js_error)?;

        // If the target channel has no per-channel snapshots yet, rebuild them
        let target_snapshots = self.repo.load_all_snapshots().map_err(to_js_error)?;
        let resource_state = if target_snapshots.is_empty() && !target_channel.changesets.is_empty() {
            let state: HashMap<String, serde_json::Value> = izip!(&target_channel.changesets)
                .filter_map(|cid| self.repo.load_changeset(cid).ok())
                .flat_map(|cs| cs.patches.into_iter())
                .fold(HashMap::new(), |mut state, p| {
                    let current_val = state
                        .entry(p.target_resource.clone())
                        .or_insert_with(|| serde_json::json!({}));
                    p.result_snapshot
                        .as_ref()
                        .map(|result| *current_val = result.clone())
                        .unwrap_or_else(|| {
                            let _ = diff::apply_patch(current_val, &p.operations);
                        });
                    state
                });
            // Persist rebuilt snapshots
            izip!(&state).try_for_each(|(resource_id, value)| {
                self.repo.save_snapshot(resource_id, value)
            }).map_err(to_js_error)?;
            state
        } else {
            target_snapshots
        };

        // Write resource files to working directory
        izip!(&resource_state).try_for_each(|(resource_id, value)| {
            serde_json::to_string_pretty(value)
                .map_err(anyhow::Error::from)
                .and_then(|json| self.repo.write_resource_file(resource_id, &json))
        }).map_err(to_js_error)?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Restore
    // -----------------------------------------------------------------------

    /// Restore a file from the current channel's snapshot.
    #[wasm_bindgen]
    pub fn restore(&self, path: &str) -> Result<(), JsError> {
        let resource_id = self.repo.resource_id_from_relative(path);
        let snapshot = self
            .repo
            .load_snapshot(&resource_id)
            .map_err(to_js_error)?
            .ok_or_else(|| JsError::new(&format!("No snapshot found for '{}'", resource_id)))?;

        serde_json::to_string_pretty(&snapshot)
            .map_err(|e| JsError::new(&e.to_string()))
            .and_then(|json| self.repo.write_resource_file(&resource_id, &json).map_err(to_js_error))
    }

    /// Restore a file from a specific channel's computed state.
    #[wasm_bindgen]
    pub fn restore_from_channel(&self, path: &str, channel_name: &str) -> Result<(), JsError> {
        let resource_id = self.repo.resource_id_from_relative(path);
        let channel = self.repo.load_channel(channel_name).map_err(to_js_error)?;

        let resource_state: HashMap<String, serde_json::Value> = izip!(&channel.changesets)
            .filter_map(|cid| self.repo.load_changeset(cid).ok())
            .flat_map(|cs| cs.patches.into_iter())
            .fold(HashMap::new(), |mut state, p| {
                let current_val = state
                    .entry(p.target_resource.clone())
                    .or_insert_with(|| serde_json::json!({}));
                p.result_snapshot
                    .as_ref()
                    .map(|result| *current_val = result.clone())
                    .unwrap_or_else(|| {
                        let _ = diff::apply_patch(current_val, &p.operations);
                    });
                state
            });

        let value = resource_state
            .get(&resource_id)
            .ok_or_else(|| JsError::new(&format!("Resource '{}' not found in channel '{}'", resource_id, channel_name)))?;

        serde_json::to_string_pretty(value)
            .map_err(|e| JsError::new(&e.to_string()))
            .and_then(|json| self.repo.write_resource_file(&resource_id, &json).map_err(to_js_error))?;

        self.repo.save_snapshot(&resource_id, value).map_err(to_js_error)
    }

    /// Restore a resource to its state in a specific changeset.
    /// Replays all changesets up to and including the given change_id,
    /// then writes the resource's state to the working directory.
    #[wasm_bindgen]
    pub fn restore_from_changeset(&self, resource_id: &str, change_id: &str) -> Result<(), JsError> {
        let channel_name = self.repo.current_channel_name().map_err(to_js_error)?;
        let channel = self.repo.load_channel(&channel_name).map_err(to_js_error)?;

        // Also check all channels for the changeset
        let all_channels = self.repo.list_channels().map_err(to_js_error)?;
        let mut found_changesets: Vec<String> = Vec::new();
        for ch in &all_channels {
            if ch.changesets.contains(&change_id.to_string()) {
                // Replay up to and including the target changeset
                for cid in &ch.changesets {
                    found_changesets.push(cid.clone());
                    if cid == change_id {
                        break;
                    }
                }
                break;
            }
        }

        if found_changesets.is_empty() {
            return Err(JsError::new(&format!("Changeset '{}' not found in any channel", change_id)));
        }

        let mut resource_state: HashMap<String, serde_json::Value> = HashMap::new();
        for cid in &found_changesets {
            if let Ok(cs) = self.repo.load_changeset(cid) {
                for p in cs.patches {
                    let current_val = resource_state
                        .entry(p.target_resource.clone())
                        .or_insert_with(|| serde_json::json!({}));
                    p.result_snapshot
                        .as_ref()
                        .map(|result| *current_val = result.clone())
                        .unwrap_or_else(|| {
                            let _ = diff::apply_patch(current_val, &p.operations);
                        });
                }
            }
        }

        let value = resource_state
            .get(resource_id)
            .ok_or_else(|| JsError::new(&format!("Resource '{}' not found in changeset '{}'", resource_id, change_id)))?;

        serde_json::to_string_pretty(value)
            .map_err(|e| JsError::new(&e.to_string()))
            .and_then(|json| self.repo.write_resource_file(resource_id, &json).map_err(to_js_error))?;

        self.repo.save_snapshot(resource_id, value).map_err(to_js_error)
    }

    /// Get a resource's snapshot from a specific channel (without modifying working dir).
    /// Returns the JSON content as a string.
    #[wasm_bindgen]
    pub fn get_snapshot_from_channel(&self, resource_id: &str, channel_name: &str) -> Result<String, JsError> {
        let channel = self.repo.load_channel(channel_name).map_err(to_js_error)?;

        let resource_state: HashMap<String, serde_json::Value> = izip!(&channel.changesets)
            .filter_map(|cid| self.repo.load_changeset(cid).ok())
            .flat_map(|cs| cs.patches.into_iter())
            .fold(HashMap::new(), |mut state, p| {
                let current_val = state
                    .entry(p.target_resource.clone())
                    .or_insert_with(|| serde_json::json!({}));
                p.result_snapshot
                    .as_ref()
                    .map(|result| *current_val = result.clone())
                    .unwrap_or_else(|| {
                        let _ = diff::apply_patch(current_val, &p.operations);
                    });
                state
            });

        let value = resource_state
            .get(resource_id)
            .ok_or_else(|| JsError::new(&format!("Resource '{}' not found in channel '{}'", resource_id, channel_name)))?;

        serde_json::to_string_pretty(value).map_err(|e| JsError::new(&e.to_string()))
    }

    /// List resource IDs available in a specific channel.
    #[wasm_bindgen]
    pub fn list_channel_resources(&self, channel_name: &str) -> Result<String, JsError> {
        let channel = self.repo.load_channel(channel_name).map_err(to_js_error)?;

        let resource_ids: Vec<String> = izip!(&channel.changesets)
            .filter_map(|cid| self.repo.load_changeset(cid).ok())
            .flat_map(|cs| cs.patches.into_iter())
            .map(|p| p.target_resource)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        serde_json::to_string(&resource_ids).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Get the log for a specific channel (not necessarily the current one).
    #[wasm_bindgen]
    pub fn log_for_channel(&self, channel_name: &str) -> Result<String, JsError> {
        let channel = self.repo.load_channel(channel_name).map_err(to_js_error)?;

        let entries: Vec<LogEntry> = izip!(&channel.changesets)
            .rev()
            .filter_map(|cid| {
                self.repo.load_changeset(cid).ok().map(|cs| LogEntry {
                    change_id: cs.change_id,
                    commit_hash: cs.commit_hash,
                    message: cs.message,
                    author: cs.author,
                    created_at: cs.created_at.to_rfc3339(),
                    patch_count: cs.patches.len(),
                    immutable: cs.immutable,
                })
            })
            .collect();

        serde_json::to_string(&entries).map_err(|e| JsError::new(&e.to_string()))
    }

    // -----------------------------------------------------------------------
    // Squash
    // -----------------------------------------------------------------------

    /// Squash the current channel's head changeset into its parent.
    /// Returns the target changeset's change_id.
    #[wasm_bindgen]
    pub fn squash(&self) -> Result<String, JsError> {
        let channel_name = self.repo.current_channel_name().map_err(to_js_error)?;
        let channel = self.repo.load_channel(&channel_name).map_err(to_js_error)?;

        if channel.changesets.len() < 2 {
            return Err(JsError::new("Cannot squash: need at least 2 changesets."));
        }

        let child_id = channel.changesets.last().unwrap().clone();
        let parent_idx = channel.changesets.len() - 2;
        let target_id = channel.changesets[parent_idx].clone();

        let child = self.repo.load_changeset(&child_id).map_err(to_js_error)?;
        let mut target = self.repo.load_changeset(&target_id).map_err(to_js_error)?;

        if child.immutable || target.immutable {
            return Err(JsError::new("Cannot squash immutable changesets."));
        }

        // Merge patches
        izip!(child.patches).for_each(|child_patch| {
            let existing = target
                .patches
                .iter_mut()
                .find(|p| p.target_resource == child_patch.target_resource);

            match existing {
                Some(target_patch) => {
                    target_patch.result_snapshot = child_patch.result_snapshot;
                    target_patch.operations = diff::diff(
                        target_patch
                            .parent_snapshot
                            .as_ref()
                            .unwrap_or(&serde_json::json!({})),
                        target_patch
                            .result_snapshot
                            .as_ref()
                            .unwrap_or(&serde_json::json!({})),
                    );
                    target_patch.hash = {
                        let content = dyna_core::models::PatchContent {
                            target_resource: target_patch.target_resource.clone(),
                            operations: target_patch.operations.clone(),
                            parent_snapshot: target_patch.parent_snapshot.clone(),
                            result_snapshot: target_patch.result_snapshot.clone(),
                        };
                        let serialized = serde_json::to_vec(&content).unwrap();
                        dyna_core::hash::content_hash(&serialized)
                    };
                }
                None => {
                    target.patches.push(child_patch);
                }
            }
        });

        target.recompute_hash();
        self.repo.save_changeset(&target).map_err(to_js_error)?;

        // Update channel: remove child
        let mut updated_channel = channel.clone();
        updated_channel.changesets.retain(|id| id != &child_id);
        updated_channel.head_change_id = updated_channel.changesets.last().cloned();
        self.repo.save_channel(&updated_channel).map_err(to_js_error)?;

        // Update working change if it was the child
        let working = self.repo.working_change_id().map_err(to_js_error)?;
        if working.as_deref() == Some(&child_id) {
            self.repo
                .set_working_change(Some(&target_id))
                .map_err(to_js_error)?;
        }

        // Remove child changeset file
        self.repo.remove_changeset_file(&child_id).map_err(to_js_error)?;

        Ok(target_id)
    }

    // -----------------------------------------------------------------------
    // Revert
    // -----------------------------------------------------------------------

    /// Revert a changeset by creating a new changeset with inverse patches.
    ///
    /// Returns the change_id of the newly created revert changeset.
    #[wasm_bindgen]
    pub fn revert(&self, change_id: &str) -> Result<String, JsError> {
        let channel_name = self.repo.current_channel_name().map_err(to_js_error)?;

        // Protect the main channel
        if channel_name == "main" {
            return Err(JsError::new(
                "Cannot revert directly on the 'main' channel. \
                 Switch to a feature channel first, then promote to main.",
            ));
        }

        let target_cs = self.repo.load_changeset(change_id).map_err(to_js_error)?;

        if target_cs.patches.is_empty() {
            return Err(JsError::new("Changeset has no patches to revert."));
        }

        let config = self.repo.load_config().map_err(to_js_error)?;
        let author = config.user.name.clone();

        // Build inverse patches
        let inverse_patches: Vec<Patch> = izip!(&target_cs.patches)
            .map(|original| {
                let base = original
                    .parent_snapshot
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));

                let inverse_ops = diff::invert_operations(&original.operations, &base);

                let content = PatchContent {
                    target_resource: original.target_resource.clone(),
                    operations: inverse_ops,
                    parent_snapshot: original.result_snapshot.clone(),
                    result_snapshot: original.parent_snapshot.clone(),
                };
                let serialized = serde_json::to_vec(&content).unwrap();
                let hash = dyna_core::hash::content_hash(&serialized);

                Patch {
                    hash,
                    target_resource: content.target_resource,
                    operations: content.operations,
                    parent_snapshot: content.parent_snapshot,
                    result_snapshot: content.result_snapshot,
                }
            })
            .collect();

        // Parent is the current channel head
        let channel = self.repo.load_channel(&channel_name).map_err(to_js_error)?;
        let parents = channel
            .head_change_id
            .clone()
            .map(|id| vec![id])
            .unwrap_or_default();

        let revert_message = format!(
            "Revert \"{}\" ({})",
            target_cs.message,
            &target_cs.change_id[..std::cmp::min(target_cs.change_id.len(), 8)]
        );

        let revert_cs = Changeset::new(author, revert_message, parents, inverse_patches);
        let revert_id = revert_cs.change_id.clone();

        // Save changeset
        self.repo.save_changeset(&revert_cs).map_err(to_js_error)?;

        // Update snapshots
        izip!(&revert_cs.patches).try_for_each(|p| {
            p.result_snapshot
                .as_ref()
                .map(|snap| {
                    if snap.is_null() {
                        self.repo.remove_snapshot(&p.target_resource)
                    } else {
                        self.repo.save_snapshot(&p.target_resource, snap)
                    }
                })
                .unwrap_or(Ok(()))
        }).map_err(to_js_error)?;

        // Update channel
        let mut channel = self.repo.load_channel(&channel_name).map_err(to_js_error)?;
        channel.append_changeset(revert_cs.change_id.clone());
        self.repo.save_channel(&channel).map_err(to_js_error)?;

        // Update working change
        self.repo
            .set_working_change(Some(&revert_id))
            .map_err(to_js_error)?;

        Ok(revert_id)
    }

    // -----------------------------------------------------------------------
    // Cherry-pick
    // -----------------------------------------------------------------------

    /// Cherry-pick a changeset from another channel onto the current channel.
    ///
    /// Copies the patches from the source changeset and applies them as a new
    /// changeset on the current channel. Returns the new change_id.
    #[wasm_bindgen]
    pub fn cherry_pick(&self, change_id: &str) -> Result<String, JsError> {
        let channel_name = self.repo.current_channel_name().map_err(to_js_error)?;

        // Protect the main channel
        if channel_name == "main" {
            return Err(JsError::new(
                "Cannot cherry-pick directly onto the 'main' channel. \
                 Switch to a feature channel first, then promote to main.",
            ));
        }

        let source_cs = self.repo.load_changeset(change_id).map_err(to_js_error)?;

        if source_cs.patches.is_empty() {
            return Err(JsError::new("Changeset has no patches to cherry-pick."));
        }

        let channel = self.repo.load_channel(&channel_name).map_err(to_js_error)?;
        if channel.changesets.contains(&source_cs.change_id) {
            return Err(JsError::new("Changeset is already in this channel."));
        }

        let config = self.repo.load_config().map_err(to_js_error)?;
        let author = config.user.name.clone();

        // Build cherry-pick patches
        let cherry_patches: Vec<Patch> = izip!(&source_cs.patches)
            .map(|src_patch| {
                let current_snapshot = self
                    .repo
                    .load_snapshot(&src_patch.target_resource)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| serde_json::json!({}));

                let new_snapshot = src_patch
                    .result_snapshot
                    .as_ref()
                    .and_then(|result| {
                        src_patch.parent_snapshot.as_ref().map(|parent| {
                            let delta_ops = diff::diff(parent, result);
                            let mut dest = current_snapshot.clone();
                            diff::apply_patch(&mut dest, &delta_ops).ok();
                            dest
                        })
                    })
                    .unwrap_or_else(|| {
                        src_patch
                            .result_snapshot
                            .clone()
                            .unwrap_or_else(|| current_snapshot.clone())
                    });

                let operations = diff::diff(&current_snapshot, &new_snapshot);

                let content = PatchContent {
                    target_resource: src_patch.target_resource.clone(),
                    operations,
                    parent_snapshot: Some(current_snapshot),
                    result_snapshot: Some(new_snapshot),
                };
                let serialized = serde_json::to_vec(&content).unwrap();
                let hash = dyna_core::hash::content_hash(&serialized);

                Patch {
                    hash,
                    target_resource: content.target_resource,
                    operations: content.operations,
                    parent_snapshot: content.parent_snapshot,
                    result_snapshot: content.result_snapshot,
                }
            })
            .collect();

        let parents = channel
            .head_change_id
            .clone()
            .map(|id| vec![id])
            .unwrap_or_default();

        let cherry_message = format!(
            "Cherry-pick \"{}\" ({})",
            source_cs.message,
            &source_cs.change_id[..std::cmp::min(source_cs.change_id.len(), 8)]
        );

        let cherry_cs = Changeset::new(author, cherry_message, parents, cherry_patches);
        let cherry_id = cherry_cs.change_id.clone();

        // Save changeset
        self.repo.save_changeset(&cherry_cs).map_err(to_js_error)?;

        // Update snapshots
        izip!(&cherry_cs.patches).try_for_each(|p| {
            p.result_snapshot
                .as_ref()
                .map(|snap| {
                    if snap.is_null() {
                        self.repo.remove_snapshot(&p.target_resource)
                    } else {
                        self.repo.save_snapshot(&p.target_resource, snap)
                    }
                })
                .unwrap_or(Ok(()))
        }).map_err(to_js_error)?;

        // Update channel
        let mut channel = self.repo.load_channel(&channel_name).map_err(to_js_error)?;
        channel.append_changeset(cherry_cs.change_id.clone());
        self.repo.save_channel(&channel).map_err(to_js_error)?;

        // Update working change
        self.repo
            .set_working_change(Some(&cherry_id))
            .map_err(to_js_error)?;

        Ok(cherry_id)
    }

    // -----------------------------------------------------------------------
    // Remote operations (async)
    // -----------------------------------------------------------------------

    /// Clone a repository from the remote server into the in-memory VFS.
    #[wasm_bindgen]
    pub async fn clone_repo(&mut self, remote_url: &str) -> Result<(), JsError> {
        self.remote_url = Some(remote_url.trim_end_matches('/').to_string());
        let client = SyncClient::new(remote_url);

        let response = client
            .clone_repo(&CloneRequest { channel: None })
            .await
            .map_err(to_js_error)?;

        // Initialise the repo
        self.repo.init().map_err(to_js_error)?;

        let mut config = self.repo.load_config().map_err(to_js_error)?;
        config.remote_url = self.remote_url.clone();
        self.repo.save_config(&config).map_err(to_js_error)?;

        // Save all channels
        izip!(&response.channels).try_for_each(|ch| {
            self.repo.save_channel(ch)
        }).map_err(to_js_error)?;

        // Set HEAD to main (or first channel)
        let head_channel = response
            .channels
            .iter()
            .find(|c| c.name == "main")
            .or_else(|| response.channels.first())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "main".to_string());
        self.repo.set_current_channel(&head_channel).map_err(to_js_error)?;

        // Save all changesets
        izip!(&response.changesets).try_for_each(|cs| {
            self.repo.save_changeset(cs)
        }).map_err(to_js_error)?;

        // Build per-channel snapshots by replaying each channel's changesets
        izip!(&response.channels).try_for_each(|channel| -> anyhow::Result<()> {
            let mut channel_snapshots: HashMap<String, serde_json::Value> = HashMap::new();
            izip!(&channel.changesets)
                .filter_map(|cid| self.repo.load_changeset(cid).ok())
                .flat_map(|cs| cs.patches.into_iter())
                .for_each(|p| {
                    let entry = channel_snapshots
                        .entry(p.target_resource.clone())
                        .or_insert_with(|| serde_json::json!({}));
                    p.result_snapshot
                        .as_ref()
                        .map(|result| *entry = result.clone())
                        .unwrap_or_else(|| {
                            let _ = diff::apply_patch(entry, &p.operations);
                        });
                });
            izip!(&channel_snapshots).try_for_each(|(resource_id, value)| {
                self.repo.save_snapshot_for_channel(&channel.name, resource_id, value)
            })
        }).map_err(to_js_error)?;

        // Write working directory files from current channel's snapshots
        izip!(&self.repo.load_all_snapshots().map_err(to_js_error)?)
            .try_for_each(|(resource_id, value)| -> anyhow::Result<()> {
                serde_json::to_string_pretty(value)
                    .map_err(anyhow::Error::from)
                    .and_then(|json| self.repo.write_resource_file(resource_id, &json))
            }).map_err(to_js_error)?;

        // Set working change to head of current channel
        let channel = self.repo.load_channel(&head_channel).map_err(to_js_error)?;
        self.repo
            .set_working_change(channel.head_change_id.as_deref())
            .map_err(to_js_error)?;

        // Save sync state: fold channel heads into sync_state
        let mut sync_state = self.repo.load_sync_state().map_err(to_js_error)?;
        izip!(&response.channels)
            .filter_map(|ch| ch.head_change_id.as_ref().map(|head| (ch.name.clone(), head.clone())))
            .for_each(|(name, head)| { sync_state.remote_heads.insert(name, head); });
        self.repo.save_sync_state(&sync_state).map_err(to_js_error)?;

        Ok(())
    }

    /// Push local changesets to the remote server.
    #[wasm_bindgen]
    pub async fn push(&self) -> Result<String, JsError> {
        let remote_url = self
            .remote_url
            .as_ref()
            .ok_or_else(|| JsError::new("No remote URL configured. Call init_repo() or set_remote() first."))?;

        let client = SyncClient::new(remote_url);
        let channel_name = self.repo.current_channel_name().map_err(to_js_error)?;

        // Protect the main channel from direct pushes
        if channel_name == "main" {
            return Err(JsError::new(
                "Cannot push directly to the 'main' channel. \
                 Push to a feature channel and use promote instead.",
            ));
        }

        let channel = self.repo.load_channel(&channel_name).map_err(to_js_error)?;
        let sync_state = self.repo.load_sync_state().map_err(to_js_error)?;

        let remote_head = sync_state.remote_heads.get(&channel_name).cloned();

        // Find changesets after the remote head
        let unpushed_ids: Vec<String> = remote_head
            .as_ref()
            .map(|head| {
                izip!(&channel.changesets)
                    .skip_while(|id| *id != head)
                    .skip(1)
                    .cloned()
                    .collect()
            })
            .unwrap_or_else(|| channel.changesets.clone());

        if unpushed_ids.is_empty() {
            return Ok(serde_json::json!({ "pushed": 0 }).to_string());
        }

        let to_push: Vec<Changeset> = unpushed_ids
            .iter()
            .filter_map(|cid| self.repo.load_changeset(cid).ok())
            .collect();

        let request = PushRequest {
            channel: channel_name.clone(),
            changesets: to_push,
            expected_head: remote_head,
        };

        let response = client.push(&request).await.map_err(to_js_error)?;

        // Update sync state
        response.new_head.as_ref().map(|new_head| -> Result<(), JsError> {
            let mut sync_state = self.repo.load_sync_state().map_err(to_js_error)?;
            sync_state.remote_heads.insert(channel_name, new_head.clone());
            self.repo.save_sync_state(&sync_state).map_err(to_js_error)
        }).transpose()?;

        serde_json::to_string(&serde_json::json!({
            "pushed": response.accepted_count,
            "new_head": response.new_head,
        }))
        .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Pull changesets from the remote server.
    #[wasm_bindgen]
    pub async fn pull(&self) -> Result<String, JsError> {
        let remote_url = self
            .remote_url
            .as_ref()
            .ok_or_else(|| JsError::new("No remote URL configured."))?;

        let client = SyncClient::new(remote_url);
        let channel_name = self.repo.current_channel_name().map_err(to_js_error)?;
        let sync_state = self.repo.load_sync_state().map_err(to_js_error)?;

        let since = sync_state.remote_heads.get(&channel_name).cloned();

        let request = PullRequest {
            channel: channel_name.clone(),
            since_change_id: since,
        };

        let response = client.pull(&request).await.map_err(to_js_error)?;
        let pulled_count = response.changesets.len();

        if response.changesets.is_empty() {
            return Ok(serde_json::json!({ "pulled": 0 }).to_string());
        }

        // Save new changesets — smart dedup: skip if already local
        let mut imported = 0usize;
        izip!(&response.changesets).try_for_each(|cs| {
            if self.repo.load_changeset(&cs.change_id).is_ok() {
                imported += 1;
                Ok(()) // already exists locally, just import by reference
            } else {
                self.repo.save_changeset(cs)
            }
        }).map_err(to_js_error)?;

        // Update local channel
        self.repo.save_channel(&response.channel).map_err(to_js_error)?;

        // Replay to update working directory
        let resource_state: HashMap<String, serde_json::Value> =
            izip!(&response.channel.changesets)
                .filter_map(|cid| self.repo.load_changeset(cid).ok())
                .flat_map(|cs| cs.patches.into_iter())
                .fold(HashMap::new(), |mut state, p| {
                    let current_val = state
                        .entry(p.target_resource.clone())
                        .or_insert_with(|| serde_json::json!({}));
                    p.result_snapshot
                        .as_ref()
                        .map(|result| *current_val = result.clone())
                        .unwrap_or_else(|| {
                            let _ = diff::apply_patch(current_val, &p.operations);
                        });
                    state
                });

        izip!(&resource_state).try_for_each(|(resource_id, value)| {
            self.repo.save_snapshot(resource_id, value)?;
            serde_json::to_string_pretty(value)
                .map_err(anyhow::Error::from)
                .and_then(|json| self.repo.write_resource_file(resource_id, &json))
        }).map_err(to_js_error)?;

        // Update sync state
        response.current_head.as_ref().map(|head| -> Result<(), JsError> {
            let mut sync_state = self.repo.load_sync_state().map_err(to_js_error)?;
            sync_state.remote_heads.insert(channel_name.clone(), head.clone());
            self.repo.save_sync_state(&sync_state).map_err(to_js_error)
        }).transpose()?;

        // Update working change
        self.repo
            .set_working_change(response.channel.head_change_id.as_deref())
            .map_err(to_js_error)?;

        serde_json::to_string(&serde_json::json!({
            "pulled": pulled_count,
            "imported": imported,
            "current_head": response.current_head,
        }))
        .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Promote changesets from a source channel to the target channel on the remote.
    #[wasm_bindgen]
    pub async fn promote(&self, source_channel: &str, target_channel: &str) -> Result<String, JsError> {
        let remote_url = self
            .remote_url
            .as_ref()
            .ok_or_else(|| JsError::new("No remote URL configured."))?;

        let client = SyncClient::new(remote_url);
        let request = PromoteRequest {
            source_channel: source_channel.to_string(),
            target_channel: target_channel.to_string(),
        };

        let response = client.promote(&request).await.map_err(to_js_error)?;

        serde_json::to_string(&serde_json::json!({
            "promoted": response.promoted_changesets.len(),
            "new_head": response.new_head,
        }))
        .map_err(|e| JsError::new(&e.to_string()))
    }

    /// List remote channels.
    #[wasm_bindgen]
    pub async fn list_remote_channels(&self) -> Result<String, JsError> {
        let remote_url = self
            .remote_url
            .as_ref()
            .ok_or_else(|| JsError::new("No remote URL configured."))?;

        let client = SyncClient::new(remote_url);
        let response = client.list_channels().await.map_err(to_js_error)?;

        let infos: Vec<ChannelInfo> = izip!(&response.channels)
            .map(|ch| ChannelInfo {
                name: ch.name.clone(),
                changeset_count: ch.changesets.len(),
                head: ch.head_change_id.clone(),
                is_current: false,
            })
            .collect();

        serde_json::to_string(&infos).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Check remote server health.
    #[wasm_bindgen]
    pub async fn health(&self) -> Result<String, JsError> {
        let remote_url = self
            .remote_url
            .as_ref()
            .ok_or_else(|| JsError::new("No remote URL configured."))?;

        let client = SyncClient::new(remote_url);
        let response = client.health().await.map_err(to_js_error)?;

        serde_json::to_string(&response).map_err(|e| JsError::new(&e.to_string()))
    }

    // -----------------------------------------------------------------------
    // Snapshot inspection (for debugging / UI)
    // -----------------------------------------------------------------------

    /// Get a resource's current snapshot as a JSON string.
    #[wasm_bindgen]
    pub fn get_snapshot(&self, resource_id: &str) -> Result<String, JsError> {
        self.repo
            .load_snapshot(resource_id)
            .map_err(to_js_error)?
            .map(|v| serde_json::to_string(&v).unwrap_or_default())
            .ok_or_else(|| JsError::new(&format!("No snapshot for '{}'", resource_id)))
    }

    /// List all resource IDs that have snapshots.
    #[wasm_bindgen]
    pub fn list_snapshots(&self) -> Result<String, JsError> {
        self.repo
            .load_all_snapshots()
            .map_err(to_js_error)
            .and_then(|snaps| {
                let ids: Vec<&String> = snaps.keys().collect();
                serde_json::to_string(&ids).map_err(|e| JsError::new(&e.to_string()))
            })
    }

    /// Get a changeset by its change_id as a JSON string.
    #[wasm_bindgen]
    pub fn get_changeset(&self, change_id: &str) -> Result<String, JsError> {
        self.repo
            .load_changeset(change_id)
            .map_err(to_js_error)
            .and_then(|cs| serde_json::to_string(&cs).map_err(|e| JsError::new(&e.to_string())))
    }

    // -----------------------------------------------------------------------
    // Resource History (remote query)
    // -----------------------------------------------------------------------

    /// Query the change history of a resource from the remote server.
    /// Returns a JSON-encoded `ResourceHistoryResponse`.
    #[wasm_bindgen]
    pub async fn resource_history(&self, resource_id: &str) -> Result<String, JsError> {
        let remote_url = self
            .remote_url
            .as_deref()
            .ok_or_else(|| JsError::new("No remote URL configured."))?;
        let url = format!("{}/api/v1/resources/{}/history", remote_url, resource_id);

        let headers = web_sys::Headers::new()
            .map_err(|e| JsError::new(&format!("Headers: {:?}", e)))?;
        headers
            .set("Accept-Encoding", "gzip")
            .map_err(|e| JsError::new(&format!("Header: {:?}", e)))?;

        let opts = web_sys::RequestInit::new();
        opts.set_method("GET");
        opts.set_headers(&headers);
        opts.set_mode(web_sys::RequestMode::Cors);

        let request = web_sys::Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| JsError::new(&format!("Request: {:?}", e)))?;

        let window = web_sys::window()
            .ok_or_else(|| JsError::new("No global window"))?;
        let resp_value =
            wasm_bindgen_futures::JsFuture::from(window.fetch_with_request(&request))
                .await
                .map_err(|e| JsError::new(&format!("Fetch: {:?}", e)))?;

        let resp: web_sys::Response = resp_value
            .dyn_into()
            .map_err(|_| JsError::new("Not a Response"))?;

        let ab = wasm_bindgen_futures::JsFuture::from(
            resp.array_buffer()
                .map_err(|e| JsError::new(&format!("Body: {:?}", e)))?,
        )
        .await
        .map_err(|e| JsError::new(&format!("Read body: {:?}", e)))?;

        let bytes = js_sys::Uint8Array::new(&ab).to_vec();
        let decompressed = dyna_core::compression::read_transparent(&bytes)
            .map_err(|e| JsError::new(&format!("Decompress: {}", e)))?;

        Ok(String::from_utf8(decompressed)
            .map_err(|e| JsError::new(&format!("UTF-8: {}", e)))?)
    }

    // -----------------------------------------------------------------------
    // WebSocket Notifications
    // -----------------------------------------------------------------------

    /// Connect to the server's WebSocket endpoint and register a JS callback
    /// that will be called with each notification JSON string.
    ///
    /// Returns a handle (the WebSocket object) that can be closed later.
    ///
    /// ```js
    /// const ws = client.subscribe_notifications((json) => {
    ///     const notification = JSON.parse(json);
    ///     console.log("Notification:", notification);
    /// });
    /// ```
    #[wasm_bindgen]
    pub fn subscribe_notifications(
        &self,
        callback: &js_sys::Function,
    ) -> Result<web_sys::WebSocket, JsError> {
        let remote = self
            .remote_url
            .as_ref()
            .ok_or_else(|| JsError::new("No remote URL configured"))?;

        // Convert http(s):// to ws(s)://
        let ws_url = remote
            .replace("https://", "wss://")
            .replace("http://", "ws://");
        let ws_url = format!("{}/api/v1/ws", ws_url);

        let ws = web_sys::WebSocket::new(&ws_url)
            .map_err(|e| JsError::new(&format!("WebSocket connect: {:?}", e)))?;

        // Set binary type to arraybuffer (we only expect text, but just in case)
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        // On message: call the JS callback with the message data (string)
        let cb = callback.clone();
        let onmessage = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
            move |event: web_sys::MessageEvent| {
                if let Some(text) = event.data().as_string() {
                    let this = JsValue::null();
                    let _ = cb.call1(&this, &JsValue::from_str(&text));
                }
            },
        );
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget(); // prevent GC

        // On error: log to console
        let onerror = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(
            move |event: web_sys::ErrorEvent| {
                web_sys::console::error_1(
                    &JsValue::from_str(&format!("WebSocket error: {:?}", event.message())),
                );
            },
        );
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        Ok(ws)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_js_error(e: anyhow::Error) -> JsError {
    JsError::new(&format!("{:#}", e))
}
