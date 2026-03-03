//! Python bindings for the Dyna distributed CRUD system.
//!
//! This crate provides a `DynaRepo` Python class that wraps the Rust
//! [`dyna_cli::repository::Repository`] and [`dyna_cli::sync_client::SyncClient`]
//! via PyO3. Every CLI command is exposed as a method on `DynaRepo`.

use std::collections::HashSet;
use std::path::PathBuf;

use itertools::izip;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use serde_json::Value;

use dyna_cli::repository::Repository;
use dyna_cli::sync_client::SyncClient;
use dyna_core::models::{
    Changeset, Patch, PatchOperation, StagedChange, SyncState,
};
use dyna_core::protocol::{
    CloneRequest, PromoteRequest, PullRequest, PushRequest,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_py_err(e: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{:#}", e))
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new()
        .expect("Failed to create tokio runtime")
        .block_on(f)
}

// ---------------------------------------------------------------------------
// DynaRepo — the main Python class
// ---------------------------------------------------------------------------

/// A Dyna repository backed by the local filesystem.
///
/// Construct with `DynaRepo(path)` where `path` is the working directory
/// containing a `.dyna/` folder. Use `DynaRepo.init()` or
/// `DynaRepo.clone_repo()` to create a new repository.
#[pyclass]
struct DynaRepo {
    repo: Repository,
}

#[pymethods]
impl DynaRepo {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    #[new]
    fn new(path: &str) -> PyResult<Self> {
        let work_dir = PathBuf::from(path);
        if !work_dir.join(".dyna").exists() {
            return Err(PyRuntimeError::new_err(format!(
                "No .dyna directory found in '{}'. Run init() or clone_repo() first.",
                path
            )));
        }
        let repo = Repository::from_work_dir(work_dir);
        Ok(Self { repo })
    }

    /// Initialise a new Dyna repository at `path`.
    #[staticmethod]
    #[pyo3(signature = (path, remote_url=None, user_name=None))]
    fn init(path: &str, remote_url: Option<&str>, user_name: Option<&str>) -> PyResult<Self> {
        let work_dir = PathBuf::from(path);
        std::fs::create_dir_all(&work_dir)
            .map_err(|e| PyRuntimeError::new_err(format!("Cannot create directory: {}", e)))?;

        // Repository::init creates .dyna/ and all subdirectories, writes HEAD,
        // default config, main channel, and sync state.
        let repo = Repository::init(&work_dir).map_err(to_py_err)?;

        // Optionally override config
        if remote_url.is_some() || user_name.is_some() {
            let mut config = repo.load_config().map_err(to_py_err)?;
            remote_url.map(|url| config.remote_url = Some(url.to_string()));
            user_name.map(|name| config.user.name = name.to_string());
            repo.save_config(&config).map_err(to_py_err)?;
        }

        Ok(Self { repo })
    }

    // -----------------------------------------------------------------------
    // Config
    // -----------------------------------------------------------------------

    /// Get the remote URL.
    fn remote_url(&self) -> PyResult<Option<String>> {
        let config = self.repo.load_config().map_err(to_py_err)?;
        Ok(config.remote_url)
    }

    /// Set the remote URL.
    fn set_remote(&self, url: &str) -> PyResult<()> {
        let mut config = self.repo.load_config().map_err(to_py_err)?;
        config.remote_url = Some(url.to_string());
        self.repo.save_config(&config).map_err(to_py_err)
    }

    /// Get the user name.
    fn user_name(&self) -> PyResult<String> {
        let config = self.repo.load_config().map_err(to_py_err)?;
        Ok(config.user.name)
    }

    /// Set the user name.
    fn set_user_name(&self, name: &str) -> PyResult<()> {
        let mut config = self.repo.load_config().map_err(to_py_err)?;
        config.user.name = name.to_string();
        self.repo.save_config(&config).map_err(to_py_err)
    }

    // -----------------------------------------------------------------------
    // File I/O (working directory)
    // -----------------------------------------------------------------------

    /// Write a JSON resource to the working directory.
    fn write_resource(&self, resource_id: &str, json_str: &str) -> PyResult<()> {
        let _: Value = serde_json::from_str(json_str)
            .map_err(|e| PyRuntimeError::new_err(format!("Invalid JSON: {}", e)))?;
        self.repo
            .write_resource_file(resource_id, json_str)
            .map_err(to_py_err)
    }

    /// Read a JSON resource from the working directory.
    fn read_resource(&self, resource_id: &str) -> PyResult<String> {
        self.repo.read_resource_file(resource_id).map_err(to_py_err)
    }

    /// Check if a resource exists in the working directory.
    fn resource_exists(&self, resource_id: &str) -> PyResult<bool> {
        let rel = self.repo.relative_path_for_resource_id(resource_id);
        self.repo.work_file_exists(&rel).map_err(to_py_err)
    }

    /// Delete a resource from the working directory.
    fn delete_resource(&self, resource_id: &str) -> PyResult<()> {
        let rel = self.repo.relative_path_for_resource_id(resource_id);
        self.repo.remove_work_file(&rel).map_err(to_py_err)
    }

    /// List all resource IDs in the working directory.
    fn list_resources(&self) -> PyResult<Vec<String>> {
        self.repo
            .list_work_json_files()
            .map_err(to_py_err)
            .map(|files| {
                files
                    .iter()
                    .map(|f| self.repo.resource_id_from_relative(f))
                    .collect()
            })
    }

    // -----------------------------------------------------------------------
    // Staging (add)
    // -----------------------------------------------------------------------

    /// Stage a resource for commit.
    fn add(&self, resource_id: &str) -> PyResult<()> {
        let rel = self.repo.relative_path_for_resource_id(resource_id);
        let current_content = self.repo.read_work_file(&rel).map_err(to_py_err)?;
        let current: Value = serde_json::from_str(&current_content)
            .map_err(|e| PyRuntimeError::new_err(format!("Invalid JSON in {}: {}", resource_id, e)))?;

        let previous = self.repo.load_snapshot(resource_id).map_err(to_py_err)?;
        let operations = match &previous {
            Some(prev) => dyna_core::diff::diff(prev, &current),
            None => dyna_core::diff::diff(&Value::Null, &current),
        };

        if operations.is_empty() {
            return Err(PyRuntimeError::new_err(format!(
                "No changes detected for '{}'",
                resource_id
            )));
        }

        let staged = StagedChange {
            resource_id: resource_id.to_string(),
            file_path: rel,
            previous,
            current,
            operations,
        };
        self.repo.stage_change(&staged).map_err(to_py_err)
    }

    /// Stage a resource deletion.
    fn add_delete(&self, resource_id: &str) -> PyResult<()> {
        let rel = self.repo.relative_path_for_resource_id(resource_id);
        let previous = self
            .repo
            .load_snapshot(resource_id)
            .map_err(to_py_err)?
            .ok_or_else(|| {
                PyRuntimeError::new_err(format!(
                    "Resource '{}' has no snapshot — nothing to delete",
                    resource_id
                ))
            })?;

        let operations = vec![PatchOperation::Remove {
            path: "/".to_string(),
        }];

        let staged = StagedChange {
            resource_id: resource_id.to_string(),
            file_path: rel,
            previous: Some(previous),
            current: Value::Null,
            operations,
        };
        self.repo.stage_change(&staged).map_err(to_py_err)
    }

    /// Unstage a previously staged resource, moving it back to the working directory.
    /// For staged deletions, restores the working directory file from the previous snapshot.
    fn unstage(&self, resource_id: &str) -> PyResult<()> {
        let staged_changes = self.repo.load_staged_changes().map_err(to_py_err)?;
        let staged = staged_changes
            .iter()
            .find(|s| s.resource_id == resource_id)
            .ok_or_else(|| {
                PyRuntimeError::new_err(format!("Resource '{}' is not staged.", resource_id))
            })?;

        // For staged deletions, restore the working directory file
        staged.current.is_null().then(|| {
            staged.previous.as_ref().map(|previous| {
                serde_json::to_string_pretty(previous)
                    .map_err(|e| PyRuntimeError::new_err(format!("JSON error: {}", e)))
                    .and_then(|content| {
                        self.repo
                            .write_resource_file(resource_id, &content)
                            .map_err(to_py_err)
                    })
            })
        });

        self.repo.remove_staging_file(resource_id).map_err(to_py_err)
    }

    /// Unstage all staged changes, moving them back to the working directory.
    /// Returns the number of resources unstaged.
    fn unstage_all(&self) -> PyResult<u32> {
        let staged_changes = self.repo.load_staged_changes().map_err(to_py_err)?;
        let count = staged_changes.len() as u32;

        izip!(&staged_changes)
            .filter(|staged| staged.current.is_null())
            .try_for_each(|staged| {
                staged.previous.as_ref().map(|previous| {
                    serde_json::to_string_pretty(previous)
                        .map_err(|e| PyRuntimeError::new_err(format!("JSON error: {}", e)))
                        .and_then(|content| {
                            self.repo
                                .write_resource_file(&staged.resource_id, &content)
                                .map_err(to_py_err)
                        })
                }).unwrap_or(Ok(()))
            })?;

        self.repo.clear_staging().map_err(to_py_err)?;
        Ok(count)
    }

    /// Check if all changesets of a channel exist in main.
    fn is_promoted_to_main(&self, channel_name: &str) -> PyResult<bool> {
        let channel = self.repo.load_channel(channel_name).map_err(to_py_err)?;
        let main = self.repo.load_channel("main").map_err(to_py_err)?;
        let main_set: std::collections::HashSet<&str> = izip!(&main.changesets).map(|s| s.as_str()).collect();
        Ok(izip!(&channel.changesets).all(|c| main_set.contains(c.as_str())))
    }

    /// Delete a local channel. Raises error if channel is 'main' or is current.
    fn delete_channel(&self, channel_name: &str, force: Option<bool>) -> PyResult<()> {
        if channel_name == "main" {
            return Err(PyRuntimeError::new_err("Cannot delete the 'main' channel: it is protected."));
        }
        let current = self.repo.current_channel_name().map_err(to_py_err)?;
        if channel_name == current {
            return Err(PyRuntimeError::new_err("Cannot delete the current channel. Switch to another channel first."));
        }
        let force = force.unwrap_or(false);
        if !force {
            let promoted = self.is_promoted_to_main(channel_name)?;
            if !promoted {
                return Err(PyRuntimeError::new_err(format!(
                    "Channel '{}' has not been fully promoted to main. Use force=True to delete anyway.",
                    channel_name
                )));
            }
        }
        self.repo.delete_channel(channel_name).map_err(to_py_err)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Status
    // -----------------------------------------------------------------------

    /// Get repository status as a dict with keys:
    /// channel, staged, modified, deleted, untracked, conflicts.
    fn status(&self) -> PyResult<PyObject> {
        Python::with_gil(|py| {
            let dict = pyo3::types::PyDict::new(py);

            let channel = self.repo.current_channel_name().map_err(to_py_err)?;
            dict.set_item("channel", &channel)?;

            let staged = self.repo.load_staged_changes().map_err(to_py_err)?;
            let staged_list: Vec<PyObject> = staged
                .iter()
                .map(|s| {
                    let d = pyo3::types::PyDict::new(py);
                    d.set_item("resource_id", &s.resource_id).unwrap();
                    d.set_item("op_count", s.operations.len()).unwrap();
                    let kind = if s.previous.is_none() {
                        "new"
                    } else if s.current.is_null() {
                        "deleted"
                    } else {
                        "modified"
                    };
                    d.set_item("kind", kind).unwrap();
                    d.into()
                })
                .collect();
            dict.set_item("staged", staged_list)?;

            let staged_ids: HashSet<String> =
                izip!(&staged).map(|s| s.resource_id.clone()).collect();

            let snapshots = self.repo.load_all_snapshots().map_err(to_py_err)?;
            let work_files = self.repo.list_work_json_files().map_err(to_py_err)?;

            // Modified (unstaged changes to tracked files)
            let modified: Vec<String> = izip!(&work_files)
                .map(|file_path| (self.repo.resource_id_from_relative(file_path), file_path))
                .filter(|(rid, _)| !staged_ids.contains(rid))
                .filter(|(rid, file_path)| {
                    snapshots.get(rid).and_then(|snapshot_val| {
                        self.repo.read_work_file(file_path).ok().and_then(|content| {
                            serde_json::from_str::<Value>(&content)
                                .ok()
                                .map(|current_val| &current_val != snapshot_val)
                        })
                    }).unwrap_or(false)
                })
                .map(|(rid, _)| rid)
                .collect();
            dict.set_item("modified", modified)?;

            // Deleted tracked files
            let work_rids: HashSet<String> = izip!(&work_files)
                .map(|f| self.repo.resource_id_from_relative(f))
                .collect();
            let deleted: Vec<String> = snapshots
                .keys()
                .filter(|rid| !work_rids.contains(*rid) && !staged_ids.contains(*rid))
                .cloned()
                .collect();
            dict.set_item("deleted", deleted)?;

            // Untracked (new files not yet in snapshots)
            let untracked: Vec<String> = izip!(&work_files)
                .map(|file_path| self.repo.resource_id_from_relative(file_path))
                .filter(|rid| !snapshots.contains_key(rid) && !staged_ids.contains(rid))
                .collect();
            dict.set_item("untracked", untracked)?;

            // Conflicts
            let conflicts = self.repo.list_conflicted_resources().map_err(to_py_err)?;
            dict.set_item("conflicts", conflicts)?;

            Ok(dict.into())
        })
    }

    // -----------------------------------------------------------------------
    // Commit
    // -----------------------------------------------------------------------

    /// Commit staged changes. Returns the change_id.
    fn commit(&self, message: &str) -> PyResult<String> {
        let staged = self.repo.load_staged_changes().map_err(to_py_err)?;
        if staged.is_empty() {
            return Err(PyRuntimeError::new_err("Nothing staged to commit"));
        }

        let config = self.repo.load_config().map_err(to_py_err)?;
        let channel_name = self.repo.current_channel_name().map_err(to_py_err)?;

        // Protect the main channel from direct commits
        if channel_name == "main" {
            return Err(PyRuntimeError::new_err(
                "Cannot commit directly to the 'main' channel. \
                 Switch to a feature channel first, then promote to main.",
            ));
        }

        let mut channel = self.repo.load_channel(&channel_name).map_err(to_py_err)?;

        let patches: Vec<Patch> = staged
            .iter()
            .map(|s| dyna_core::patch::build_patch(s))
            .collect();

        let parents = channel
            .head_change_id
            .clone()
            .map(|id| vec![id])
            .unwrap_or_default();

        let changeset = Changeset::new(
            config.user.name.clone(),
            message.to_string(),
            parents,
            patches,
        );

        let change_id = changeset.change_id.clone();

        self.repo.store_changeset(&changeset).map_err(to_py_err)?;
        channel.changesets.push(changeset.change_id.clone());
        channel.head_change_id = Some(changeset.change_id.clone());
        self.repo.save_channel(&channel).map_err(to_py_err)?;

        self.repo
            .set_working_change(Some(&change_id))
            .map_err(to_py_err)?;

        izip!(&staged).try_for_each(|s| {
            if s.current.is_null() {
                self.repo.remove_snapshot(&s.resource_id).map_err(to_py_err)
            } else {
                self.repo.save_snapshot(&s.resource_id, &s.current).map_err(to_py_err)
            }
        })?;

        self.repo.clear_staging().map_err(to_py_err)?;
        Ok(change_id)
    }

    // -----------------------------------------------------------------------
    // Push
    // -----------------------------------------------------------------------

    /// Push local changesets to the remote server.
    #[pyo3(signature = (channel=None))]
    fn push(&self, channel: Option<&str>) -> PyResult<PyObject> {
        let config = self.repo.load_config().map_err(to_py_err)?;
        let remote_url = config
            .remote_url
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("No remote URL configured"))?
            .clone();

        let channel_name = channel
            .map(|c| c.to_string())
            .unwrap_or_else(|| self.repo.current_channel_name().unwrap_or_default());

        // Protect the main channel from direct pushes
        if channel_name == "main" {
            return Err(PyRuntimeError::new_err(
                "Cannot push directly to the 'main' channel. \
                 Push to a feature channel and use promote instead.",
            ));
        }

        let ch = self.repo.load_channel(&channel_name).map_err(to_py_err)?;
        let sync_state = self.repo.load_sync_state().map_err(to_py_err)?;

        let pushed_set: HashSet<&str> = sync_state
            .pushed_changesets
            .iter()
            .map(|s| s.as_str())
            .collect();
        let new_changeset_ids: Vec<&String> = ch
            .changesets
            .iter()
            .filter(|id| !pushed_set.contains(id.as_str()))
            .collect();

        if new_changeset_ids.is_empty() {
            return Err(PyRuntimeError::new_err(
                "Nothing to push — all changesets are up to date",
            ));
        }

        let new_changesets: Vec<Changeset> = new_changeset_ids
            .iter()
            .filter_map(|id| self.repo.load_changeset(id).ok())
            .collect();

        let expected_head = sync_state.remote_heads.get(&channel_name).cloned();

        let request = PushRequest {
            channel: channel_name.clone(),
            changesets: new_changesets,
            expected_head,
        };

        let client = SyncClient::new(&remote_url);
        let response = block_on(client.push(&request)).map_err(to_py_err)?;

        if response.success {
            let mut new_sync = sync_state;
            response.new_head.as_ref().map(|new_head| {
                new_sync
                    .remote_heads
                    .insert(channel_name.clone(), new_head.clone());
            });
            let new_ids: Vec<String> = izip!(&new_changeset_ids)
                .filter(|id| !new_sync.pushed_changesets.contains(*id))
                .map(|id| (*id).clone())
                .collect();
            new_sync.pushed_changesets.extend(new_ids);
            self.repo.save_sync_state(&new_sync).map_err(to_py_err)?;
        }

        Python::with_gil(|py| {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("success", response.success)?;
            dict.set_item("changesets_pushed", response.accepted_count)?;
            dict.set_item("channel", &channel_name)?;
            response.error.as_ref().map(|err| dict.set_item("error", err)).transpose()?;
            Ok(dict.into())
        })
    }

    // -----------------------------------------------------------------------
    // Pull
    // -----------------------------------------------------------------------

    /// Pull changesets from the remote server.
    #[pyo3(signature = (channel=None))]
    fn pull(&self, channel: Option<&str>) -> PyResult<PyObject> {
        let config = self.repo.load_config().map_err(to_py_err)?;
        let remote_url = config
            .remote_url
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("No remote URL configured"))?
            .clone();

        let channel_name = channel
            .map(|c| c.to_string())
            .unwrap_or_else(|| self.repo.current_channel_name().unwrap_or_default());

        let sync_state = self.repo.load_sync_state().map_err(to_py_err)?;
        let since = sync_state.remote_heads.get(&channel_name).cloned();

        let request = PullRequest {
            channel: channel_name.clone(),
            since_change_id: since,
        };

        let client = SyncClient::new(&remote_url);
        let response = block_on(client.pull(&request)).map_err(to_py_err)?;
        let new_count = response.changesets.len();

        izip!(&response.changesets).try_for_each(|cs| {
            self.repo.store_changeset(cs).map_err(to_py_err)
        })?;

        self.repo
            .save_channel(&response.channel)
            .map_err(to_py_err)?;

        let mut new_sync = sync_state;
        response.current_head.as_ref().map(|head| {
            new_sync
                .remote_heads
                .insert(channel_name.clone(), head.clone());
        });
        self.repo.save_sync_state(&new_sync).map_err(to_py_err)?;

        let resources_updated = izip!(&response.changesets)
            .flat_map(|cs| izip!(&cs.patches))
            .try_fold(0usize, |count, patch| -> PyResult<usize> {
                patch.result_snapshot.as_ref().map(|snap| {
                    serde_json::to_string_pretty(snap)
                        .map_err(|e| to_py_err(e.into()))
                        .and_then(|json| {
                            self.repo
                                .write_resource_file(&patch.target_resource, &json)
                                .map_err(to_py_err)
                        })
                        .and_then(|_| {
                            self.repo
                                .save_snapshot(&patch.target_resource, snap)
                                .map_err(to_py_err)
                        })
                        .map(|_| count + 1)
                }).unwrap_or(Ok(count))
            })?;

        Python::with_gil(|py| {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("changesets_pulled", new_count)?;
            dict.set_item("channel", &channel_name)?;
            dict.set_item("resources_updated", resources_updated)?;
            Ok(dict.into())
        })
    }

    // -----------------------------------------------------------------------
    // Promote
    // -----------------------------------------------------------------------

    /// Promote changesets from a channel to main.
    #[pyo3(signature = (channel=None))]
    fn promote(&self, channel: Option<&str>) -> PyResult<PyObject> {
        let config = self.repo.load_config().map_err(to_py_err)?;
        let remote_url = config
            .remote_url
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("No remote URL configured"))?
            .clone();

        let source_channel = channel
            .map(|c| c.to_string())
            .unwrap_or_else(|| self.repo.current_channel_name().unwrap_or_default());

        if source_channel == "main" {
            return Err(PyRuntimeError::new_err(
                "Cannot promote from 'main'. Specify a source channel.",
            ));
        }

        let request = PromoteRequest {
            source_channel: source_channel.clone(),
            target_channel: "main".to_string(),
        };

        let client = SyncClient::new(&remote_url);
        let response = block_on(client.promote(&request)).map_err(to_py_err)?;

        Python::with_gil(|py| {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("success", response.success)?;
            dict.set_item("source_channel", &source_channel)?;
            dict.set_item("promoted_count", response.promoted_changesets.len())?;
            response.new_head.as_ref().map(|new_head| dict.set_item("new_head", new_head)).transpose()?;
            response.error.as_ref().map(|err| dict.set_item("error", err)).transpose()?;
            Ok(dict.into())
        })
    }

    /// Perform a local promote from source channel to target channel.
    /// Returns a dict with 'success', 'promoted_count', and optionally 'conflicts'.
    /// Conflicts are returned as a list of dicts with 'resource_id', 'json_path',
    /// 'base_value', 'local_value', 'remote_value'.
    #[pyo3(signature = (source_channel, target_channel="main"))]
    fn promote_local(&self, source_channel: &str, target_channel: &str) -> PyResult<PyObject> {
        use dyna_core::channel::promote_changesets;
        use dyna_core::diff::three_way_merge_checked;

        let source = self.repo.load_channel(source_channel).map_err(to_py_err)?;
        let mut target = self.repo.load_channel(target_channel).map_err(to_py_err)?;

        let promoted = promote_changesets(&source, &mut target)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let mut all_conflicts: Vec<(String, Vec<dyna_core::models::Conflict>)> = Vec::new();

        for cs_id in izip!(&promoted) {
            let cs = self.repo.load_changeset(cs_id).map_err(to_py_err)?;
            for p in izip!(&cs.patches) {
                let resource_id = &p.target_resource;

                let base = p.parent_snapshot.clone().unwrap_or(serde_json::Value::Null);
                let local = self.repo
                    .load_snapshot_for_channel(target_channel, resource_id)
                    .map_err(to_py_err)?
                    .unwrap_or(serde_json::Value::Null);
                let remote = p.result_snapshot.clone().unwrap_or(serde_json::Value::Null);

                // Fast path: new resource (base is null and target is null)
                if base.is_null() && local.is_null() {
                    self.repo.save_snapshot_for_channel(target_channel, resource_id, &remote)
                        .map_err(to_py_err)?;
                    continue;
                }

                // Fast path: no divergence on target
                if local == base {
                    self.repo.save_snapshot_for_channel(target_channel, resource_id, &remote)
                        .map_err(to_py_err)?;
                    continue;
                }

                match three_way_merge_checked(&base, &local, &remote) {
                    Ok(merged) => {
                        self.repo.save_snapshot_for_channel(target_channel, resource_id, &merged)
                            .map_err(to_py_err)?;
                    }
                    Err(conflicts) => {
                        self.repo.save_conflicts(resource_id, &conflicts)
                            .map_err(to_py_err)?;
                        all_conflicts.push((resource_id.clone(), conflicts));
                    }
                }
            }
        }

        Python::with_gil(|py| {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("promoted_count", promoted.len())?;

            if all_conflicts.is_empty() {
                dict.set_item("success", true)?;
                // Update target channel
                target.head_change_id = source.head_change_id.clone();
                self.repo.save_channel(&target).map_err(to_py_err)?;
            } else {
                dict.set_item("success", false)?;
                let conflict_list = pyo3::types::PyList::empty(py);
                for (resource_id, conflicts) in izip!(&all_conflicts) {
                    for c in izip!(conflicts) {
                        let cd = pyo3::types::PyDict::new(py);
                        cd.set_item("resource_id", resource_id)?;
                        cd.set_item("json_path", &c.json_path)?;
                        cd.set_item("base_value", c.base_value.as_ref().map(|v| v.to_string()))?;
                        cd.set_item("local_value", c.local_value.to_string())?;
                        cd.set_item("remote_value", c.remote_value.to_string())?;
                        conflict_list.append(cd)?;
                    }
                }
                dict.set_item("conflicts", conflict_list)?;
            }

            Ok(dict.into())
        })
    }

    // -----------------------------------------------------------------------
    // Clone
    // -----------------------------------------------------------------------

    /// Clone a remote repository into `path`.
    #[staticmethod]
    #[pyo3(signature = (url, path, user_name=None))]
    fn clone_repo(url: &str, path: &str, user_name: Option<&str>) -> PyResult<Self> {
        let work_dir = PathBuf::from(path);
        std::fs::create_dir_all(&work_dir)
            .map_err(|e| PyRuntimeError::new_err(format!("Cannot create directory: {}", e)))?;

        let repo = Repository::init(&work_dir).map_err(to_py_err)?;

        let mut config = repo.load_config().map_err(to_py_err)?;
        config.remote_url = Some(url.to_string());
        user_name.map(|name| config.user.name = name.to_string());
        repo.save_config(&config).map_err(to_py_err)?;

        let client = SyncClient::new(url);
        let request = CloneRequest { channel: None };
        let response = block_on(client.clone_repo(&request)).map_err(to_py_err)?;

        izip!(&response.changesets).try_for_each(|cs| {
            repo.store_changeset(cs).map_err(to_py_err)
        })?;
        izip!(&response.channels).try_for_each(|ch| {
            repo.save_channel(ch).map_err(to_py_err)
        })?;
        izip!(&response.snapshots).try_for_each(|(resource_id, snapshot)| {
            serde_json::to_string_pretty(snapshot)
                .map_err(|e| to_py_err(e.into()))
                .and_then(|json| {
                    repo.write_resource_file(resource_id, &json)
                        .map_err(to_py_err)
                })
                .and_then(|_| {
                    repo.save_snapshot(resource_id, snapshot)
                        .map_err(to_py_err)
                })
        })?;

        izip!(&response.channels)
            .find(|c| c.name == "main")
            .map(|c| c)
            .map(|main_ch| -> PyResult<()> {
                repo.set_current_channel(&main_ch.name).map_err(to_py_err)?;
                main_ch.head_change_id.as_ref().map(|head| {
                    repo.set_working_change(Some(head)).map_err(to_py_err)
                }).transpose()?;
                Ok(())
            })
            .or_else(|| {
                response.channels.first().map(|first_ch| {
                    repo.set_current_channel(&first_ch.name).map_err(to_py_err)
                })
            })
            .transpose()?;

        let sync_state = izip!(&response.channels).fold(SyncState::default(), |mut state, ch| {
            ch.head_change_id.as_ref().map(|head| {
                state.remote_heads.insert(ch.name.clone(), head.clone());
            });
            let new_ids: Vec<String> = izip!(&ch.changesets)
                .filter(|cs_id| !state.pushed_changesets.contains(cs_id))
                .cloned()
                .collect();
            state.pushed_changesets.extend(new_ids);
            state
        });
        repo.save_sync_state(&sync_state).map_err(to_py_err)?;

        Ok(Self { repo })
    }

    // -----------------------------------------------------------------------
    // Channels
    // -----------------------------------------------------------------------

    /// Get the current channel name.
    fn current_channel(&self) -> PyResult<String> {
        self.repo.current_channel_name().map_err(to_py_err)
    }

    /// List all local channels. Returns a list of channel names.
    fn list_channels(&self) -> PyResult<Vec<String>> {
        self.repo
            .list_channels()
            .map(|channels| izip!(&channels).map(|c| c.name.clone()).collect())
            .map_err(to_py_err)
    }

    /// Create a new channel, optionally forking from an existing one.
    #[pyo3(signature = (name, fork_from=None))]
    fn create_channel(&self, name: &str, fork_from: Option<&str>) -> PyResult<()> {
        self.repo
            .create_channel(name, fork_from)
            .map_err(to_py_err)?;
        Ok(())
    }

    /// Switch to a channel.
    fn switch_channel(&self, name: &str) -> PyResult<()> {
        let _ch = self.repo.load_channel(name).map_err(to_py_err)?;
        self.repo.set_current_channel(name).map_err(to_py_err)
    }

    // -----------------------------------------------------------------------
    // Log
    // -----------------------------------------------------------------------

    /// Get the changeset log. Returns a list of dicts.
    #[pyo3(signature = (count=None, verbose=None))]
    fn log(&self, count: Option<usize>, verbose: Option<bool>) -> PyResult<Vec<PyObject>> {
        let channel_name = self.repo.current_channel_name().map_err(to_py_err)?;
        let changesets = self
            .repo
            .load_channel_changesets(&channel_name)
            .map_err(to_py_err)?;
        let verbose = verbose.unwrap_or(false);
        let limit = count.unwrap_or(changesets.len());

        Python::with_gil(|py| {
            changesets
                .iter()
                .rev()
                .take(limit)
                .map(|cs| {
                    let dict = pyo3::types::PyDict::new(py);
                    dict.set_item("change_id", &cs.change_id)?;
                    dict.set_item("commit_hash", &cs.commit_hash)?;
                    dict.set_item("message", &cs.message)?;
                    dict.set_item("author", &cs.author)?;
                    dict.set_item("created_at", cs.created_at.to_rfc3339())?;
                    dict.set_item("parents", &cs.parents)?;
                    dict.set_item("patch_count", cs.patches.len())?;
                    dict.set_item("immutable", cs.immutable)?;

                    if verbose {
                        let patches: Vec<PyObject> = cs
                            .patches
                            .iter()
                            .map(|p| {
                                let pd = pyo3::types::PyDict::new(py);
                                pd.set_item("target_resource", &p.target_resource)
                                    .unwrap();
                                pd.set_item("hash", &p.hash).unwrap();
                                pd.set_item("op_count", p.operations.len()).unwrap();
                                pd.into()
                            })
                            .collect();
                        dict.set_item("patches", patches)?;
                    }

                    Ok(dict.into())
                })
                .collect()
        })
    }

    // -----------------------------------------------------------------------
    // Diff
    // -----------------------------------------------------------------------

    /// Compute diff for a resource. Returns JSON string of operations.
    fn diff(&self, resource_id: &str) -> PyResult<String> {
        let rel = self.repo.relative_path_for_resource_id(resource_id);
        let current_content = self.repo.read_work_file(&rel).map_err(to_py_err)?;
        let current: Value = serde_json::from_str(&current_content)
            .map_err(|e| PyRuntimeError::new_err(format!("Invalid JSON: {}", e)))?;

        let previous = self
            .repo
            .load_snapshot(resource_id)
            .map_err(to_py_err)?
            .unwrap_or(Value::Null);

        let ops = dyna_core::diff::diff(&previous, &current);
        serde_json::to_string_pretty(&ops).map_err(|e| to_py_err(e.into()))
    }

    // -----------------------------------------------------------------------
    // Restore
    // -----------------------------------------------------------------------

    /// Restore a resource to its snapshot state.
    #[pyo3(signature = (resource_id, _channel=None, changeset=None))]
    fn restore(
        &self,
        resource_id: &str,
        _channel: Option<&str>,
        changeset: Option<&str>,
    ) -> PyResult<()> {
        let snapshot_value = changeset.map(|cs_prefix| {
            let matches = self
                .repo
                .find_changeset_by_prefix(cs_prefix)
                .map_err(to_py_err)?;
            match matches.len() {
                0 => Err(PyRuntimeError::new_err(format!(
                    "No changeset found matching '{}'",
                    cs_prefix
                ))),
                1 => {
                    let cs = &matches[0];
                    izip!(&cs.patches)
                        .find(|p| p.target_resource == resource_id)
                        .and_then(|p| p.result_snapshot.clone())
                        .ok_or_else(|| {
                            PyRuntimeError::new_err(format!(
                                "Changeset '{}' does not contain resource '{}'",
                                cs.change_id, resource_id
                            ))
                        })
                }
                _ => {
                    let ids: Vec<&str> =
                        izip!(&matches).map(|m| m.change_id.as_str()).collect();
                    Err(PyRuntimeError::new_err(format!(
                        "Ambiguous prefix '{}', matches: {:?}",
                        cs_prefix, ids
                    )))
                }
            }
        }).unwrap_or_else(|| {
            self.repo
                .load_snapshot(resource_id)
                .map_err(to_py_err)?
                .ok_or_else(|| {
                    PyRuntimeError::new_err(format!(
                        "No snapshot found for '{}'",
                        resource_id
                    ))
                })
        })?;

        let json =
            serde_json::to_string_pretty(&snapshot_value).map_err(|e| to_py_err(e.into()))?;
        self.repo
            .write_resource_file(resource_id, &json)
            .map_err(to_py_err)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Squash
    // -----------------------------------------------------------------------

    /// Squash a changeset into its parent. Returns the target change_id.
    #[pyo3(signature = (revision=None, into=None, message=None))]
    fn squash(
        &self,
        revision: Option<&str>,
        into: Option<&str>,
        message: Option<&str>,
    ) -> PyResult<String> {
        let channel_name = self.repo.current_channel_name().map_err(to_py_err)?;
        let mut channel = self.repo.load_channel(&channel_name).map_err(to_py_err)?;

        let child_id = revision.map(|rev| {
            let matches = self
                .repo
                .find_changeset_by_prefix(rev)
                .map_err(to_py_err)?;
            match matches.len() {
                0 => Err(PyRuntimeError::new_err(format!(
                    "No changeset found matching '{}'",
                    rev
                ))),
                1 => Ok(matches[0].change_id.clone()),
                _ => Err(PyRuntimeError::new_err("Ambiguous changeset prefix")),
            }
        }).unwrap_or_else(|| {
            channel
                .head_change_id
                .clone()
                .ok_or_else(|| {
                    PyRuntimeError::new_err("No changesets in channel to squash")
                })
        })?;

        let child = self.repo.load_changeset(&child_id).map_err(to_py_err)?;
        if child.immutable {
            return Err(PyRuntimeError::new_err(
                "Cannot squash an immutable changeset",
            ));
        }

        let target_id = into.map(|into_prefix| {
            let matches = self
                .repo
                .find_changeset_by_prefix(into_prefix)
                .map_err(to_py_err)?;
            match matches.len() {
                0 => Err(PyRuntimeError::new_err(format!(
                    "No changeset found matching '{}'",
                    into_prefix
                ))),
                1 => Ok(matches[0].change_id.clone()),
                _ => Err(PyRuntimeError::new_err("Ambiguous changeset prefix")),
            }
        }).unwrap_or_else(|| {
            child
                .parents
                .first()
                .cloned()
                .ok_or_else(|| {
                    PyRuntimeError::new_err("Changeset has no parent to squash into")
                })
        })?;

        let mut target = self.repo.load_changeset(&target_id).map_err(to_py_err)?;
        if target.immutable {
            return Err(PyRuntimeError::new_err(
                "Cannot squash into an immutable changeset",
            ));
        }

        // Merge patches
        let mut merged_patches = target.patches.clone();
        for child_patch in izip!(&child.patches) {
            if let Some(existing) = izip!(&mut merged_patches)
                .find(|p| p.target_resource == child_patch.target_resource)
                .map(|p| p)
            {
                let parent_snap = existing.parent_snapshot.clone();
                let result_snap = child_patch.result_snapshot.clone();
                let ops = match (&parent_snap, &result_snap) {
                    (Some(prev), Some(curr)) => dyna_core::diff::diff(prev, curr),
                    _ => child_patch.operations.clone(),
                };
                *existing = Patch::new(
                    existing.target_resource.clone(),
                    ops,
                    parent_snap,
                    result_snap,
                );
            } else {
                merged_patches.push(child_patch.clone());
            }
        }

        target.patches = merged_patches;
        message.map(|msg| target.message = msg.to_string());
        target.recompute_hash();

        self.repo.store_changeset(&target).map_err(to_py_err)?;

        channel.changesets.retain(|id| id != &child_id);
        if channel.head_change_id.as_deref() == Some(&child_id) {
            channel.head_change_id = channel.changesets.last().cloned();
        }
        self.repo.save_channel(&channel).map_err(to_py_err)?;

        // Update working change if needed
        self.repo.working_change_id()
            .ok()
            .flatten()
            .filter(|wc| wc == &child_id)
            .map(|_| self.repo.set_working_change(Some(&target_id)).map_err(to_py_err))
            .transpose()?;

        // Reparent any changesets that had child as parent
        let all_ids = self.repo.all_changeset_ids().map_err(to_py_err)?;
        izip!(&all_ids)
            .filter(|id| *id != &child_id && *id != &target_id)
            .try_for_each(|id| {
                self.repo.load_changeset(id).ok()
                    .filter(|cs| cs.parents.contains(&child_id))
                    .map(|mut cs| {
                        cs.parents = izip!(&cs.parents)
                            .map(|p| {
                                (p == &child_id)
                                    .then(|| target_id.clone())
                                    .unwrap_or_else(|| p.clone())
                            })
                            .collect();
                        cs.recompute_hash();
                        self.repo.store_changeset(&cs).map_err(to_py_err)
                    })
                    .unwrap_or(Ok(()))
            })?;

        self.repo
            .remove_changeset_file(&child_id)
            .map_err(to_py_err)?;
        Ok(target_id)
    }

    // -----------------------------------------------------------------------
    // Revert
    // -----------------------------------------------------------------------

    /// Revert a changeset by creating a new changeset with inverse patches.
    /// Returns the change_id of the revert changeset.
    #[pyo3(signature = (change_id, channel=None))]
    fn revert(&self, change_id: &str, channel: Option<&str>) -> PyResult<String> {
        let channel_name = channel
            .map(|c| c.to_string())
            .unwrap_or_else(|| self.repo.current_channel_name().unwrap_or_default());

        // Protect the main channel
        if channel_name == "main" {
            return Err(PyRuntimeError::new_err(
                "Cannot revert directly on the 'main' channel. \
                 Switch to a feature channel first, then promote to main.",
            ));
        }

        // Resolve changeset by prefix
        let matches = self
            .repo
            .find_changeset_by_prefix(change_id)
            .map_err(to_py_err)?;
        let target_cs = match matches.len() {
            0 => {
                return Err(PyRuntimeError::new_err(format!(
                    "No changeset found matching '{}'",
                    change_id
                )))
            }
            1 => izip!(matches).next().map(|cs| cs).unwrap(),
            n => {
                return Err(PyRuntimeError::new_err(format!(
                    "Ambiguous prefix '{}' matches {} changesets",
                    change_id, n
                )))
            }
        };

        if target_cs.patches.is_empty() {
            return Err(PyRuntimeError::new_err(
                "Changeset has no patches to revert.",
            ));
        }

        let config = self.repo.load_config().map_err(to_py_err)?;

        // Build inverse patches
        let inverse_patches: Vec<Patch> = izip!(target_cs.patches)
            .map(|original| {
                let base = original
                    .parent_snapshot
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));

                let inverse_ops =
                    dyna_core::diff::invert_operations(&original.operations, &base);

                let content = dyna_core::models::PatchContent {
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
        let ch = self
            .repo
            .load_channel(&channel_name)
            .map_err(to_py_err)?;
        let parents = ch
            .head_change_id
            .clone()
            .map(|id| vec![id])
            .unwrap_or_default();

        let revert_message = format!(
            "Revert \"{}\" ({})",
            target_cs.message,
            &target_cs.change_id[..std::cmp::min(target_cs.change_id.len(), 8)]
        );

        let revert_cs = Changeset::new(
            config.user.name.clone(),
            revert_message,
            parents,
            inverse_patches,
        );
        let revert_id = revert_cs.change_id.clone();

        self.repo.store_changeset(&revert_cs).map_err(to_py_err)?;

        // Update snapshots
        izip!(&revert_cs.patches).try_for_each(|p| {
            p.result_snapshot.as_ref().map(|snap| {
                if snap.is_null() {
                    self.repo.remove_snapshot(&p.target_resource).map_err(to_py_err)
                } else {
                    self.repo.save_snapshot(&p.target_resource, snap).map_err(to_py_err)
                }
            }).unwrap_or(Ok(()))
        })?;

        // Update channel
        let mut channel = self
            .repo
            .load_channel(&channel_name)
            .map_err(to_py_err)?;
        channel.changesets.push(revert_cs.change_id.clone());
        channel.head_change_id = Some(revert_cs.change_id.clone());
        self.repo.save_channel(&channel).map_err(to_py_err)?;

        // Set as working change
        self.repo
            .set_working_change(Some(&revert_id))
            .map_err(to_py_err)?;

        Ok(revert_id)
    }

    // -----------------------------------------------------------------------
    // Cherry-pick
    // -----------------------------------------------------------------------

    /// Cherry-pick a changeset from another channel onto the current (or specified) channel.
    /// Returns the change_id of the new cherry-pick changeset.
    #[pyo3(signature = (change_id, channel=None))]
    fn cherry_pick(&self, change_id: &str, channel: Option<&str>) -> PyResult<String> {
        let dest_channel_name = channel
            .map(|c| c.to_string())
            .unwrap_or_else(|| self.repo.current_channel_name().unwrap_or_default());

        // Protect the main channel
        if dest_channel_name == "main" {
            return Err(PyRuntimeError::new_err(
                "Cannot cherry-pick directly onto the 'main' channel. \
                 Switch to a feature channel first, then promote to main.",
            ));
        }

        // Resolve changeset by prefix
        let matches = self
            .repo
            .find_changeset_by_prefix(change_id)
            .map_err(to_py_err)?;
        let source_cs = match matches.len() {
            0 => {
                return Err(PyRuntimeError::new_err(format!(
                    "No changeset found matching '{}'",
                    change_id
                )))
            }
            1 => izip!(matches).next().map(|cs| cs).unwrap(),
            n => {
                return Err(PyRuntimeError::new_err(format!(
                    "Ambiguous prefix '{}' matches {} changesets",
                    change_id, n
                )))
            }
        };

        if source_cs.patches.is_empty() {
            return Err(PyRuntimeError::new_err(
                "Changeset has no patches to cherry-pick.",
            ));
        }

        let dest_channel = self
            .repo
            .load_channel(&dest_channel_name)
            .map_err(to_py_err)?;
        if dest_channel.changesets.contains(&source_cs.change_id) {
            return Err(PyRuntimeError::new_err(
                "Changeset is already in this channel.",
            ));
        }

        let config = self.repo.load_config().map_err(to_py_err)?;

        // Build cherry-pick patches
        let cherry_patches: Vec<Patch> = izip!(source_cs.patches)
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
                            let delta_ops = dyna_core::diff::diff(parent, result);
                            let mut dest = current_snapshot.clone();
                            dyna_core::diff::apply_patch(&mut dest, &delta_ops).ok();
                            dest
                        })
                    })
                    .unwrap_or_else(|| {
                        src_patch
                            .result_snapshot
                            .clone()
                            .unwrap_or_else(|| current_snapshot.clone())
                    });

                let operations = dyna_core::diff::diff(&current_snapshot, &new_snapshot);

                let content = dyna_core::models::PatchContent {
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

        let parents = dest_channel
            .head_change_id
            .clone()
            .map(|id| vec![id])
            .unwrap_or_default();

        let cherry_message = format!(
            "Cherry-pick \"{}\" ({})",
            source_cs.message,
            &source_cs.change_id[..std::cmp::min(source_cs.change_id.len(), 8)]
        );

        let cherry_cs = Changeset::new(
            config.user.name.clone(),
            cherry_message,
            parents,
            cherry_patches,
        );
        let cherry_id = cherry_cs.change_id.clone();

        self.repo.store_changeset(&cherry_cs).map_err(to_py_err)?;

        // Update snapshots
        izip!(&cherry_cs.patches).try_for_each(|p| {
            p.result_snapshot.as_ref().map(|snap| {
                if snap.is_null() {
                    self.repo.remove_snapshot(&p.target_resource).map_err(to_py_err)
                } else {
                    self.repo.save_snapshot(&p.target_resource, snap).map_err(to_py_err)
                }
            }).unwrap_or(Ok(()))
        })?;

        // Update channel
        let mut channel = self
            .repo
            .load_channel(&dest_channel_name)
            .map_err(to_py_err)?;
        channel.changesets.push(cherry_cs.change_id.clone());
        channel.head_change_id = Some(cherry_cs.change_id.clone());
        self.repo.save_channel(&channel).map_err(to_py_err)?;

        // Set as working change
        self.repo
            .set_working_change(Some(&cherry_id))
            .map_err(to_py_err)?;

        Ok(cherry_id)
    }

    // -----------------------------------------------------------------------
    // Describe
    // -----------------------------------------------------------------------

    /// Update the message of a changeset.
    fn describe(&self, change_id: &str, message: &str) -> PyResult<()> {
        let matches = self
            .repo
            .find_changeset_by_prefix(change_id)
            .map_err(to_py_err)?;
        match matches.len() {
            0 => Err(PyRuntimeError::new_err(format!(
                "No changeset found matching '{}'",
                change_id
            ))),
            1 => {
                let mut cs = izip!(matches).next().map(|cs| cs).unwrap();
                if cs.immutable {
                    return Err(PyRuntimeError::new_err(
                        "Cannot describe an immutable changeset",
                    ));
                }
                cs.message = message.to_string();
                cs.recompute_hash();
                self.repo.store_changeset(&cs).map_err(to_py_err)
            }
            _ => Err(PyRuntimeError::new_err("Ambiguous changeset prefix")),
        }
    }

    // -----------------------------------------------------------------------
    // History (remote query)
    // -----------------------------------------------------------------------

    /// Query the change history of a resource from the remote server.
    fn history(&self, resource_id: &str) -> PyResult<Vec<PyObject>> {
        let config = self.repo.load_config().map_err(to_py_err)?;
        let remote_url = config
            .remote_url
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("No remote URL configured"))?
            .clone();

        let client = SyncClient::new(&remote_url);
        let response =
            block_on(client.resource_history(resource_id)).map_err(to_py_err)?;

        Python::with_gil(|py| {
            response
                .entries
                .iter()
                .map(|entry| {
                    let dict = pyo3::types::PyDict::new(py);
                    dict.set_item("change_id", &entry.change_id)?;
                    dict.set_item("commit_hash", &entry.commit_hash)?;
                    dict.set_item("message", &entry.message)?;
                    dict.set_item("author", &entry.author)?;
                    dict.set_item("timestamp", &entry.timestamp)?;
                    dict.set_item("channel", &entry.channel)?;
                    let ops_json = serde_json::to_string(&entry.operations)
                        .unwrap_or_else(|_| "[]".to_string());
                    dict.set_item("operations", ops_json)?;
                    Ok(dict.into())
                })
                .collect()
        })
    }

    // -----------------------------------------------------------------------
    // Conflict resolution
    // -----------------------------------------------------------------------

    /// List conflicted resources.
    fn list_conflicts(&self) -> PyResult<Vec<String>> {
        self.repo.list_conflicted_resources().map_err(to_py_err)
    }

    /// Resolve a conflict by accepting the current working file.
    fn resolve(&self, resource_id: &str) -> PyResult<()> {
        let rel = self.repo.relative_path_for_resource_id(resource_id);
        let content = self.repo.read_work_file(&rel).map_err(to_py_err)?;
        let value: Value = serde_json::from_str(&content)
            .map_err(|e| PyRuntimeError::new_err(format!("Invalid JSON: {}", e)))?;

        self.repo
            .save_snapshot(resource_id, &value)
            .map_err(to_py_err)?;
        self.repo
            .clear_conflicts(resource_id)
            .map_err(to_py_err)
    }

    // -----------------------------------------------------------------------
    // Utility
    // -----------------------------------------------------------------------

    /// Convert a relative file path to a resource ID.
    fn path_to_resource_id(&self, path: &str) -> String {
        self.repo.resource_id_from_relative(path)
    }

    /// Convert a resource ID to a relative file path.
    fn resource_id_to_path(&self, resource_id: &str) -> String {
        self.repo.relative_path_for_resource_id(resource_id)
    }

    /// Get the working directory path.
    fn work_dir(&self) -> String {
        self.repo.work_dir.display().to_string()
    }
}

// ---------------------------------------------------------------------------
// Python module
// ---------------------------------------------------------------------------

/// The `dyna_py` Python module — Python bindings for the Dyna distributed
/// CRUD system.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<DynaRepo>()?;
    Ok(())
}
