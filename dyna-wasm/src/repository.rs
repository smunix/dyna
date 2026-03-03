//! Local repository state management for the WASM environment.
//!
//! This is a direct port of `dyna-cli/src/repository.rs`, replacing
//! `PhysicalFS` with `MemoryFS` so the entire `.dyna/` tree lives in
//! browser memory. The public API is identical, allowing all command
//! logic to be shared.
//!
//! Key difference from the CLI version:
//! - `from_work_dir` / `find` / `find_current` are removed (no physical FS).
//! - `new()` creates a fresh `MemoryFS`-backed repository.
//! - `init_in_memory()` initialises the `.dyna/` structure inside the MemoryFS.

use anyhow::{Context, Result, bail};
use dyna_core::compression;
use dyna_core::error::DynaError;
use dyna_core::models::*;
use itertools::{izip, Itertools};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use vfs::{MemoryFS, VfsPath};

const DYNA_DIR: &str = ".dyna";

// ---------------------------------------------------------------------------
// VfsPath helpers (identical to dyna-cli)
// ---------------------------------------------------------------------------

fn collect_vfs_json_entries<T, F>(dir: &VfsPath, map_fn: F) -> Result<Vec<T>>
where
    F: Fn(VfsPath) -> Result<T>,
{
    dir.exists()
        .map_err(anyhow::Error::from)
        .and_then(|exists| {
            exists
                .then(|| {
                    dir.walk_dir()
                        .map_err(anyhow::Error::from)
                        .and_then(|entries| {
                            entries
                                .filter_map(|p| p.ok())
                                .filter(|p| p.extension().map_or(false, |ext| ext == "json"))
                                .map(|p| map_fn(p))
                                .try_collect()
                        })
                })
                .unwrap_or_else(|| Ok(Vec::new()))
        })
}

fn collect_vfs_json_stems(dir: &VfsPath) -> Result<Vec<String>> {
    collect_vfs_json_entries(dir, |path| {
        let fname = path.filename();
        fname
            .strip_suffix(".json")
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Invalid file stem for {}", fname))
    })
}

fn vfs_write_bytes(path: &VfsPath, data: &[u8]) -> Result<()> {
    path.parent()
        .create_dir_all()
        .map_err(anyhow::Error::from)
        .and_then(|()| path.create_file().map_err(anyhow::Error::from))
        .and_then(|mut writer| writer.write_all(data).map_err(Into::into))
}

fn vfs_write(path: &VfsPath, content: &str) -> Result<()> {
    compression::compress_str(content)
        .map_err(anyhow::Error::from)
        .and_then(|compressed| vfs_write_bytes(path, &compressed))
}

/// Write a string to a VfsPath as plain text (no compression).
/// Use this for working directory files so they are human-readable on disk.
fn vfs_write_plain(path: &VfsPath, content: &str) -> Result<()> {
    vfs_write_bytes(path, content.as_bytes())
}

fn vfs_read_bytes(path: &VfsPath) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    path.open_file()
        .map_err(anyhow::Error::from)
        .and_then(|mut reader| {
            std::io::Read::read_to_end(&mut reader, &mut buf).map_err(Into::into)
        })
        .map(|_| buf)
}

fn vfs_read(path: &VfsPath) -> Result<String> {
    vfs_read_bytes(path).and_then(|data| {
        compression::read_transparent_str(&data).map_err(Into::into)
    })
}

fn vfs_exists(path: &VfsPath) -> Result<bool> {
    path.exists().map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Repository (MemoryFS-backed)
// ---------------------------------------------------------------------------

/// In-memory Dyna repository for the WASM environment.
///
/// All state lives inside a `MemoryFS` VFS tree. The `work_dir` PathBuf is
/// a synthetic path used only for resource-ID computation and display.
pub struct Repository {
    pub work_dir: PathBuf,
    pub dyna_dir: PathBuf,
    pub vfs_root: VfsPath,
    pub vfs_dyna: VfsPath,
}

impl Repository {
    /// Create a new in-memory repository (not yet initialised).
    pub fn new() -> Self {
        let vfs_root: VfsPath = MemoryFS::new().into();
        let vfs_dyna = vfs_root.join(DYNA_DIR).expect("join .dyna");
        Self {
            work_dir: PathBuf::from("/"),
            dyna_dir: PathBuf::from("/.dyna"),
            vfs_root,
            vfs_dyna,
        }
    }

    /// Create a repository wrapping an existing VfsPath (for advanced use).
    pub fn from_vfs(vfs_root: VfsPath) -> Self {
        let vfs_dyna = vfs_root.join(DYNA_DIR).expect("join .dyna");
        Self {
            work_dir: PathBuf::from(vfs_root.as_str()),
            dyna_dir: PathBuf::from(vfs_dyna.as_str()),
            vfs_root,
            vfs_dyna,
        }
    }

    /// Check whether this repository has been initialised.
    pub fn is_initialized(&self) -> bool {
        vfs_exists(&self.vfs_dyna).unwrap_or(false)
    }

    /// Initialise the `.dyna/` directory structure inside the MemoryFS.
    pub fn init(&self) -> Result<()> {
        vfs_exists(&self.vfs_dyna)
            .and_then(|exists| {
                if exists {
                    bail!(DynaError::AlreadyInitialized("(memory)".to_string()));
                }
                Ok(())
            })?;

        izip!(&["patches", "changesets", "staging", "channels", "snapshots", "snapshots/main", "conflicts"])
            .try_for_each(|sub| {
                self.vfs_dyna
                    .join(sub)
                    .map_err(anyhow::Error::from)
                    .and_then(|p| p.create_dir_all().map_err(Into::into))
            })?;

        vfs_write(&self.vfs_dyna.join("HEAD")?, "main")?;
        vfs_write(&self.vfs_dyna.join("WORKING_CHANGE")?, "")?;

        toml::to_string_pretty(&RepoConfig::default())
            .map_err(anyhow::Error::from)
            .and_then(|config_str| vfs_write(&self.vfs_dyna.join("config.toml")?, &config_str))?;

        serde_json::to_string_pretty(&Channel::new("main"))
            .map_err(anyhow::Error::from)
            .and_then(|json| vfs_write(&self.vfs_dyna.join("channels")?.join("main.json")?, &json))?;

        serde_json::to_string_pretty(&SyncState::default())
            .map_err(anyhow::Error::from)
            .and_then(|json| vfs_write(&self.vfs_dyna.join("sync_state.json")?, &json))?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Configuration
    // -----------------------------------------------------------------------

    pub fn load_config(&self) -> Result<RepoConfig> {
        vfs_read(&self.vfs_dyna.join("config.toml")?)
            .context("Failed to read config.toml")
            .and_then(|content| toml::from_str(&content).map_err(Into::into))
    }

    pub fn save_config(&self, config: &RepoConfig) -> Result<()> {
        toml::to_string_pretty(config)
            .map_err(Into::into)
            .and_then(|s| vfs_write(&self.vfs_dyna.join("config.toml")?, &s))
    }

    // -----------------------------------------------------------------------
    // HEAD / Current Channel
    // -----------------------------------------------------------------------

    pub fn current_channel_name(&self) -> Result<String> {
        vfs_read(&self.vfs_dyna.join("HEAD")?)
            .context("Failed to read HEAD")
            .map(|head| head.trim().to_string())
    }

    pub fn set_current_channel(&self, name: &str) -> Result<()> {
        vfs_write(&self.vfs_dyna.join("HEAD")?, name)
    }

    // -----------------------------------------------------------------------
    // Working Change
    // -----------------------------------------------------------------------

    pub fn working_change_id(&self) -> Result<Option<String>> {
        let path = self.vfs_dyna.join("WORKING_CHANGE")?;
        vfs_exists(&path)?
            .then(|| {
                vfs_read(&path)
                    .map(|s| s.trim().to_string())
                    .map(|s| (!s.is_empty()).then_some(s))
            })
            .unwrap_or(Ok(None))
    }

    pub fn set_working_change(&self, change_id: Option<&str>) -> Result<()> {
        vfs_write(
            &self.vfs_dyna.join("WORKING_CHANGE")?,
            change_id.unwrap_or(""),
        )
    }

    // -----------------------------------------------------------------------
    // Channel CRUD
    // -----------------------------------------------------------------------

    pub fn load_channel(&self, name: &str) -> Result<Channel> {
        let path = self.vfs_dyna.join("channels")?.join(&format!("{}.json", name))?;
        vfs_read(&path)
            .context(format!("Channel '{}' not found", name))
            .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
    }

    pub fn save_channel(&self, channel: &Channel) -> Result<()> {
        serde_json::to_string_pretty(channel)
            .map_err(Into::into)
            .and_then(|json| {
                vfs_write(
                    &self.vfs_dyna.join("channels")?.join(&format!("{}.json", channel.name))?,
                    &json,
                )
            })
    }

    pub fn list_channels(&self) -> Result<Vec<Channel>> {
        collect_vfs_json_entries(&self.vfs_dyna.join("channels")?, |path| {
            vfs_read(&path).and_then(|content| serde_json::from_str(&content).map_err(Into::into))
        })
    }

    pub fn create_channel(&self, name: &str, fork_from: Option<&str>) -> Result<()> {
        let channel_path = self.vfs_dyna.join("channels")?.join(&format!("{}.json", name))?;
        vfs_exists(&channel_path)?
            .then(|| -> Result<()> {
                bail!("Channel '{}' already exists", name);
            })
            .transpose()?;

        let channel = fork_from
            .map(|source| {
                self.load_channel(source).map(|src| {
                    let mut ch = Channel::new(name);
                    ch.changesets = src.changesets.clone();
                    ch.head_change_id = src.head_change_id.clone();
                    ch
                })
            })
            .unwrap_or_else(|| Ok(Channel::new(name)))?;

        // Copy snapshots from source channel to the new channel
        if let Some(source_name) = fork_from {
            if let Ok(src_dir) = self.snapshot_dir_for(source_name) {
                if let Ok(entries) = src_dir.read_dir() {
                    let dst_dir = self.snapshot_dir_for(name)?;
                    for entry in entries {
                        if entry.extension().map_or(false, |ext| ext == "json") {
                            if let Ok(content) = vfs_read(&entry) {
                                let dst = dst_dir.join(&entry.filename())?;
                                vfs_write(&dst, &content).ok();
                            }
                        }
                    }
                }
            }
        }

        self.save_channel(&channel)
    }

    /// Delete a channel: remove the channel JSON file and its snapshot directory.
    pub fn delete_channel(&self, name: &str) -> Result<()> {
        // Remove channel JSON
        let channel_path = self.vfs_dyna.join("channels")?.join(&format!("{}.json", name))?;
        if vfs_exists(&channel_path)? {
            channel_path.remove_file()?;
        }
        // Remove snapshot directory for this channel
        if let Ok(snap_dir) = self.snapshot_dir_for(name) {
            if let Ok(entries) = snap_dir.read_dir() {
                for entry in entries {
                    entry.remove_file().ok();
                }
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Changeset CRUD
    // -----------------------------------------------------------------------

    pub fn save_changeset(&self, changeset: &Changeset) -> Result<()> {
        serde_json::to_string_pretty(changeset)
            .map_err(Into::into)
            .and_then(|json| {
                vfs_write(
                    &self.vfs_dyna.join("changesets")?.join(&format!("{}.json", changeset.change_id))?,
                    &json,
                )
            })
    }

    pub fn load_changeset(&self, change_id: &str) -> Result<Changeset> {
        let path = self.vfs_dyna.join("changesets")?.join(&format!("{}.json", change_id))?;
        vfs_read(&path)
            .context(format!("Changeset '{}' not found", change_id))
            .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
    }

    pub fn list_changeset_ids(&self) -> Result<Vec<String>> {
        collect_vfs_json_stems(&self.vfs_dyna.join("changesets")?)
    }

    pub fn find_changeset_by_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        self.list_changeset_ids().map(|ids| {
            ids.into_iter()
                .filter(|id| id.starts_with(prefix))
                .collect()
        })
    }

    pub fn remove_changeset_file(&self, change_id: &str) -> Result<()> {
        let path = self.vfs_dyna.join("changesets")?.join(&format!("{}.json", change_id))?;
        vfs_exists(&path)?
            .then(|| path.remove_file().map_err(Into::into))
            .unwrap_or(Ok(()))
    }

    // -----------------------------------------------------------------------
    // Patch storage
    // -----------------------------------------------------------------------

    pub fn save_patch(&self, patch: &dyna_core::models::Patch) -> Result<()> {
        serde_json::to_string_pretty(patch)
            .map_err(Into::into)
            .and_then(|json| {
                vfs_write(
                    &self.vfs_dyna.join("patches")?.join(&format!("{}.json", patch.hash))?,
                    &json,
                )
            })
    }

    pub fn load_patch(&self, hash: &str) -> Result<dyna_core::models::Patch> {
        let path = self.vfs_dyna.join("patches")?.join(&format!("{}.json", hash))?;
        vfs_read(&path)
            .context(format!("Patch '{}' not found", hash))
            .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
    }

    // -----------------------------------------------------------------------
    // Staging
    // -----------------------------------------------------------------------

    pub fn stage_change(&self, staged: &StagedChange) -> Result<()> {
        serde_json::to_string_pretty(staged)
            .map_err(Into::into)
            .and_then(|json| {
                vfs_write(
                    &self.vfs_dyna.join("staging")?.join(&format!("{}.json", staged.resource_id))?,
                    &json,
                )
            })
    }

    pub fn load_staged_changes(&self) -> Result<Vec<StagedChange>> {
        collect_vfs_json_entries(&self.vfs_dyna.join("staging")?, |path| {
            vfs_read(&path).and_then(|content| serde_json::from_str(&content).map_err(Into::into))
        })
    }

    pub fn clear_staging(&self) -> Result<()> {
        let staging_dir = self.vfs_dyna.join("staging")?;
        vfs_exists(&staging_dir)?
            .then(|| {
                staging_dir
                    .read_dir()
                    .map_err(anyhow::Error::from)
                    .and_then(|mut entries| {
                        entries.try_for_each(|entry| entry.remove_file().map_err(Into::into))
                    })
            })
            .unwrap_or(Ok(()))
    }

    pub fn remove_staging_file(&self, resource_id: &str) -> Result<()> {
        let path = self.vfs_dyna.join("staging")?.join(&format!("{}.json", resource_id))?;
        vfs_exists(&path)?
            .then(|| path.remove_file().map_err(Into::into))
            .unwrap_or(Ok(()))
    }

    // -----------------------------------------------------------------------
    // Snapshots
    // -----------------------------------------------------------------------

    /// Return the snapshot directory for the current channel.
    fn snapshot_dir(&self) -> Result<VfsPath> {
        let channel = self.current_channel_name()?;
        let dir = self.vfs_dyna.join("snapshots")?.join(&channel)?;
        dir.create_dir_all().ok();
        Ok(dir)
    }

    /// Return the snapshot directory for a specific channel.
    fn snapshot_dir_for(&self, channel: &str) -> Result<VfsPath> {
        let dir = self.vfs_dyna.join("snapshots")?.join(channel)?;
        dir.create_dir_all().ok();
        Ok(dir)
    }

    pub fn save_snapshot(&self, resource_id: &str, value: &Value) -> Result<()> {
        serde_json::to_string_pretty(value)
            .map_err(Into::into)
            .and_then(|json| {
                vfs_write(
                    &self.snapshot_dir()?.join(&format!("{}.json", resource_id))?,
                    &json,
                )
            })
    }

    /// Save a snapshot for a specific channel.
    pub fn save_snapshot_for_channel(&self, channel: &str, resource_id: &str, value: &Value) -> Result<()> {
        serde_json::to_string_pretty(value)
            .map_err(Into::into)
            .and_then(|json| {
                vfs_write(
                    &self.snapshot_dir_for(channel)?.join(&format!("{}.json", resource_id))?,
                    &json,
                )
            })
    }

    pub fn load_snapshot(&self, resource_id: &str) -> Result<Option<Value>> {
        let path = self.snapshot_dir()?.join(&format!("{}.json", resource_id))?;
        vfs_exists(&path)?
            .then(|| {
                vfs_read(&path)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
                    .map(Some)
            })
            .unwrap_or(Ok(None))
    }

    pub fn load_all_snapshots(&self) -> Result<HashMap<String, Value>> {
        let dir = self.snapshot_dir()?;
        vfs_exists(&dir)?
            .then(|| {
                dir.read_dir()
                    .map_err(anyhow::Error::from)
                    .and_then(|entries| {
                        entries
                            .filter(|p| p.extension().map_or(false, |ext| ext == "json"))
                            .map(|p| {
                                let key = p
                                    .filename()
                                    .strip_suffix(".json")
                                    .unwrap_or_default()
                                    .to_string();
                                vfs_read(&p)
                                    .and_then(|content| {
                                        serde_json::from_str::<Value>(&content)
                                            .map_err(Into::into)
                                            .map(|value| (key, value))
                                    })
                            })
                            .try_collect()
                    })
            })
            .unwrap_or_else(|| Ok(HashMap::new()))
    }

    pub fn remove_snapshot(&self, resource_id: &str) -> Result<()> {
        let path = self.snapshot_dir()?.join(&format!("{}.json", resource_id))?;
        vfs_exists(&path)?
            .then(|| path.remove_file().map_err(Into::into))
            .unwrap_or(Ok(()))
    }

    // -----------------------------------------------------------------------
    // Sync State
    // -----------------------------------------------------------------------

    pub fn load_sync_state(&self) -> Result<SyncState> {
        let path = self.vfs_dyna.join("sync_state.json")?;
        vfs_exists(&path)?
            .then(|| {
                vfs_read(&path)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
            })
            .unwrap_or_else(|| Ok(SyncState::default()))
    }

    pub fn save_sync_state(&self, state: &SyncState) -> Result<()> {
        serde_json::to_string_pretty(state)
            .map_err(Into::into)
            .and_then(|json| vfs_write(&self.vfs_dyna.join("sync_state.json")?, &json))
    }

    // -----------------------------------------------------------------------
    // Conflicts
    // -----------------------------------------------------------------------

    pub fn save_conflicts(&self, resource_id: &str, conflicts: &[Conflict]) -> Result<()> {
        serde_json::to_string_pretty(conflicts)
            .map_err(Into::into)
            .and_then(|json| {
                vfs_write(
                    &self.vfs_dyna.join("conflicts")?.join(&format!("{}.json", resource_id))?,
                    &json,
                )
            })
    }

    pub fn load_conflicts(&self, resource_id: &str) -> Result<Vec<Conflict>> {
        let path = self.vfs_dyna.join("conflicts")?.join(&format!("{}.json", resource_id))?;
        vfs_exists(&path)?
            .then(|| {
                vfs_read(&path)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
            })
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    pub fn list_conflicted_resources(&self) -> Result<Vec<String>> {
        collect_vfs_json_stems(&self.vfs_dyna.join("conflicts")?)
    }

    pub fn clear_conflicts(&self, resource_id: &str) -> Result<()> {
        let path = self.vfs_dyna.join("conflicts")?.join(&format!("{}.json", resource_id))?;
        vfs_exists(&path)?
            .then(|| path.remove_file().map_err(Into::into))
            .unwrap_or(Ok(()))
    }

    // -----------------------------------------------------------------------
    // VFS-based working directory I/O
    // -----------------------------------------------------------------------

    pub fn read_work_file(&self, relative_path: &str) -> Result<String> {
        vfs_read(&self.vfs_root.join(relative_path)?)
    }

    pub fn write_work_file(&self, relative_path: &str, content: &str) -> Result<()> {
        let path = self.vfs_root.join(relative_path)?;
        path.parent().create_dir_all().map_err(anyhow::Error::from)?;
        // Write working directory files as plain text (not compressed)
        // so they are human-readable on disk
        vfs_write_plain(&path, content)
    }

    pub fn work_file_exists(&self, relative_path: &str) -> Result<bool> {
        vfs_exists(&self.vfs_root.join(relative_path)?)
    }

    pub fn remove_work_file(&self, relative_path: &str) -> Result<()> {
        let path = self.vfs_root.join(relative_path)?;
        vfs_exists(&path)?
            .then(|| path.remove_file().map_err(Into::into))
            .unwrap_or(Ok(()))
    }

    pub fn list_work_json_files(&self) -> Result<Vec<String>> {
        let mut results = Vec::new();
        self.collect_work_json(&self.vfs_root, &mut results)?;
        results.sort();
        Ok(results)
    }

    fn collect_work_json(&self, dir: &VfsPath, results: &mut Vec<String>) -> Result<()> {
        dir.read_dir()
            .map_err(anyhow::Error::from)
            .and_then(|mut entries| {
                entries.try_for_each(|entry| {
                    let fname = entry.filename();
                    if fname.starts_with('.') {
                        return Ok(());
                    }
                    let is_dir = entry.is_dir().unwrap_or(false);
                    if is_dir {
                        self.collect_work_json(&entry, results)
                    } else if fname.ends_with(".json") {
                        let full = entry.as_str();
                        let relative = full.strip_prefix('/').unwrap_or(full);
                        results.push(relative.to_string());
                        Ok(())
                    } else {
                        Ok(())
                    }
                })
            })
    }

    pub fn list_work_json_files_under(&self, relative_dir: &str) -> Result<Vec<String>> {
        let dir = self.vfs_root.join(relative_dir)?;
        dir.exists()
            .map_err(anyhow::Error::from)
            .and_then(|exists| {
                exists
                    .then(|| {
                        let mut results = Vec::new();
                        self.collect_work_json(&dir, &mut results)?;
                        results.sort();
                        Ok(results)
                    })
                    .unwrap_or_else(|| Ok(Vec::new()))
            })
    }

    // -----------------------------------------------------------------------
    // Resource ID helpers
    // -----------------------------------------------------------------------

    pub fn resource_id_from_relative(&self, relative_path: &str) -> String {
        relative_path
            .strip_suffix(".json")
            .unwrap_or(relative_path)
            .replace('/', ".")
    }

    pub fn relative_path_for_resource_id(&self, resource_id: &str) -> String {
        let relative = resource_id.replace('.', "/");
        format!("{}.json", relative)
    }

    pub fn path_for_resource_id(&self, resource_id: &str) -> PathBuf {
        let relative = resource_id.replace('.', "/");
        self.work_dir.join(format!("{}.json", relative))
    }

    pub fn write_resource_file(&self, resource_id: &str, content: &str) -> Result<()> {
        let relative = self.relative_path_for_resource_id(resource_id);
        self.write_work_file(&relative, content)
    }

    pub fn read_resource_file(&self, resource_id: &str) -> Result<String> {
        let relative = self.relative_path_for_resource_id(resource_id);
        self.read_work_file(&relative)
    }
}
