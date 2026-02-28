//! Local repository state management.
//!
//! Manages the `.dyna/` directory structure, which serves as the local
//! repository for the Dyna distributed CRUD system. The storage layout is
//! **changeset-centric**, inspired by Jujutsu:
//!
//! ```text
//! .dyna/
//!   config.toml              -- Repository configuration (remote URL, user)
//!   HEAD                     -- Current channel name
//!   WORKING_CHANGE           -- change_id of the working-copy changeset (@)
//!   channels/<name>.json     -- Channel metadata (ordered changeset list)
//!   changesets/<id>.json     -- Changeset objects (keyed by change_id)
//!   patches/<hash>.json      -- Individual patch objects (content-addressed)
//!   staging/<resource>.json  -- Staged changes awaiting commit
//!   snapshots/<resource>.json-- Resource snapshots for diff computation
//!   conflicts/<resource>.json-- Conflict records for unresolved merges
//!   sync/remote_head         -- Last known remote HEAD for push/pull
//! ```
//!
//! All filesystem I/O is performed through the [`vfs`] crate's [`VfsPath`]
//! abstraction, allowing the repository to operate on physical filesystems,
//! in-memory filesystems (for testing), or any other VFS implementation.
//!
//! Key operations:
//! - **Staging**: `stage_change()` records a diff between the working file and
//!   its snapshot, storing it in `staging/`.
//! - **Committing**: `commit_changeset()` creates a new [`Changeset`] from all
//!   staged changes, assigns a stable `change_id`, computes a `commit_hash`,
//!   and appends it to the current channel.
//! - **Channel switching**: `switch_channel()` checks for uncommitted staged
//!   files, cleans the working directory, and restores files from the target
//!   channel's changeset history.

use anyhow::{Context, Result, bail};
use dyna_core::error::DynaError;
use dyna_core::models::*;
use itertools::{izip, Itertools};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use vfs::{PhysicalFS, VfsPath};

const DYNA_DIR: &str = ".dyna";

// ---------------------------------------------------------------------------
// VfsPath helpers
// ---------------------------------------------------------------------------

/// Helper to read a VFS directory, filter JSON files, and collect results via a
/// mapping function. Uses iterator chains with `try_fold` semantics.
fn collect_vfs_json_entries<T, F>(dir: &VfsPath, map_fn: F) -> Result<Vec<T>>
where
    F: Fn(VfsPath) -> Result<T>,
{
    dir.exists()
        .map_err(anyhow::Error::from)
        .and_then(|exists| {
            exists
                .then(|| {
                    dir.read_dir()
                        .map_err(anyhow::Error::from)
                        .and_then(|entries| {
                            entries
                                .filter(|p| p.extension().map_or(false, |ext| ext == "json"))
                                .map(|p| map_fn(p))
                                .try_collect()
                        })
                })
                .unwrap_or_else(|| Ok(Vec::new()))
        })
}

/// Helper to read JSON file stems from a VFS directory.
fn collect_vfs_json_stems(dir: &VfsPath) -> Result<Vec<String>> {
    collect_vfs_json_entries(dir, |path| {
        let fname = path.filename();
        fname
            .strip_suffix(".json")
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Invalid file stem for {}", fname))
    })
}

/// Write a string to a VfsPath, creating the file (and overwriting if it exists).
fn vfs_write(path: &VfsPath, content: &str) -> Result<()> {
    path.create_file()
        .map_err(anyhow::Error::from)
        .and_then(|mut writer| writer.write_all(content.as_bytes()).map_err(Into::into))
}

/// Read a VfsPath to a string.
fn vfs_read(path: &VfsPath) -> Result<String> {
    path.read_to_string().map_err(Into::into)
}

/// Check if a VfsPath exists (maps VfsError to anyhow).
fn vfs_exists(path: &VfsPath) -> Result<bool> {
    path.exists().map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

/// Represents a local Dyna repository.
///
/// The `vfs_root` field is the VFS root anchored at the working directory.
/// All file I/O goes through `vfs_root` (for working-directory files) or
/// `vfs_dyna` (for `.dyna/` metadata files). The `work_dir` and `dyna_dir`
/// PathBuf fields are retained for path computation and display purposes
/// (e.g., `resource_id_from_path`), but are **not** used for I/O.
pub struct Repository {
    /// The working directory (project root) — used for path computation only.
    pub work_dir: PathBuf,
    /// The `.dyna` metadata directory — used for path computation only.
    pub dyna_dir: PathBuf,
    /// VFS root anchored at the working directory.
    pub vfs_root: VfsPath,
    /// VFS path for the `.dyna` metadata directory.
    pub vfs_dyna: VfsPath,
}

impl Repository {
    // -----------------------------------------------------------------------
    // Discovery & Initialization
    // -----------------------------------------------------------------------

    /// Create a Repository from a physical path, setting up VFS roots.
    fn from_work_dir(work_dir: PathBuf) -> Self {
        let dyna_dir = work_dir.join(DYNA_DIR);
        let vfs_root: VfsPath = PhysicalFS::new(&work_dir).into();
        let vfs_dyna = vfs_root.join(DYNA_DIR).expect("join .dyna");
        Self {
            work_dir,
            dyna_dir,
            vfs_root,
            vfs_dyna,
        }
    }

    /// Create a Repository from an existing VfsPath root (for testing with MemoryFS).
    pub fn from_vfs(vfs_root: VfsPath) -> Self {
        let vfs_dyna = vfs_root.join(DYNA_DIR).expect("join .dyna");
        Self {
            work_dir: PathBuf::from(vfs_root.as_str()),
            dyna_dir: PathBuf::from(vfs_dyna.as_str()),
            vfs_root,
            vfs_dyna,
        }
    }

    /// Find an existing repository by walking up from `start_path`.
    ///
    /// Uses `std::iter::successors` to generate parent paths, then finds the
    /// first one containing a `.dyna` directory.
    pub fn find(start_path: &Path) -> Result<Self> {
        std::iter::successors(Some(start_path.to_path_buf()), |current| {
            current.parent().map(|p| p.to_path_buf())
        })
        .find(|current| current.join(DYNA_DIR).is_dir())
        .map(|current| Self::from_work_dir(current))
        .ok_or_else(|| anyhow::anyhow!(DynaError::NotInitialized))
    }

    /// Find the repository starting from the current working directory.
    pub fn find_current() -> Result<Self> {
        std::env::current_dir()
            .context("Failed to get current directory")
            .and_then(|cwd| Self::find(&cwd))
    }

    /// Initialize a new repository at the given path.
    pub fn init(path: &Path) -> Result<Self> {
        let repo = Self::from_work_dir(path.to_path_buf());

        vfs_exists(&repo.vfs_dyna)
            .and_then(|exists| {
                if exists {
                    bail!(DynaError::AlreadyInitialized(path.display().to_string()));
                }
                Ok(())
            })?;

        // Create all subdirectories via iterator
        izip!(&["patches", "changesets", "staging", "channels", "snapshots", "conflicts"])
            .try_for_each(|sub| {
                repo.vfs_dyna
                    .join(sub)
                    .map_err(anyhow::Error::from)
                    .and_then(|p| p.create_dir_all().map_err(Into::into))
            })?;

        // HEAD points to the current channel
        vfs_write(&repo.vfs_dyna.join("HEAD")?, "main")?;

        // No working change yet
        vfs_write(&repo.vfs_dyna.join("WORKING_CHANGE")?, "")?;

        // Default config
        toml::to_string_pretty(&RepoConfig::default())
            .map_err(anyhow::Error::from)
            .and_then(|config_str| vfs_write(&repo.vfs_dyna.join("config.toml")?, &config_str))?;

        // Create the default "main" channel
        serde_json::to_string_pretty(&Channel::new("main"))
            .map_err(anyhow::Error::from)
            .and_then(|json| vfs_write(&repo.vfs_dyna.join("channels")?.join("main.json")?, &json))?;

        // Default sync state
        serde_json::to_string_pretty(&SyncState::default())
            .map_err(anyhow::Error::from)
            .and_then(|json| vfs_write(&repo.vfs_dyna.join("sync_state.json")?, &json))?;

        Ok(repo)
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
    // Working Change (the @ changeset)
    // -----------------------------------------------------------------------

    /// Get the change_id of the current working-copy changeset, if any.
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

    /// Set the working-copy changeset.
    pub fn set_working_change(&self, change_id: Option<&str>) -> Result<()> {
        vfs_write(
            &self.vfs_dyna.join("WORKING_CHANGE")?,
            change_id.unwrap_or(""),
        )
    }

    // -----------------------------------------------------------------------
    // Channel Management
    // -----------------------------------------------------------------------

    pub fn load_channel(&self, name: &str) -> Result<Channel> {
        let path = self.vfs_dyna.join("channels")?.join(&format!("{}.json", name))?;
        vfs_exists(&path)?
            .then(|| {
                vfs_read(&path)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
            })
            .unwrap_or_else(|| bail!(DynaError::ChannelNotFound(name.to_string())))
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
            vfs_read(&path)
                .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
        })
        .map(|mut channels: Vec<Channel>| {
            channels.sort_by(|a, b| a.name.cmp(&b.name));
            channels
        })
    }

    pub fn create_channel(&self, name: &str, fork_from: Option<&str>) -> Result<Channel> {
        let path = self.vfs_dyna.join("channels")?.join(&format!("{}.json", name))?;
        if vfs_exists(&path)? {
            bail!("Channel '{}' already exists", name);
        }

        let channel = fork_from
            .map(|source_name| {
                self.load_channel(source_name).map(|source| {
                    let mut new_channel = Channel::new(name);
                    new_channel.changesets = source.changesets.clone();
                    new_channel.head_change_id = source.head_change_id.clone();
                    new_channel
                })
            })
            .unwrap_or_else(|| Ok(Channel::new(name)))?;

        self.save_channel(&channel).map(|()| channel)
    }

    // -----------------------------------------------------------------------
    // Changeset Management
    // -----------------------------------------------------------------------

    /// Store a changeset in the local repository.
    ///
    /// Stores the changeset JSON and each of its patches via `try_for_each`.
    pub fn store_changeset(&self, cs: &Changeset) -> Result<()> {
        serde_json::to_string_pretty(cs)
            .map_err(Into::into)
            .and_then(|json| {
                vfs_write(
                    &self.vfs_dyna.join("changesets")?.join(&format!("{}.json", cs.change_id))?,
                    &json,
                )
            })
            .and_then(|()| {
                izip!(&cs.patches).try_for_each(|patch| self.store_patch(patch))
            })
    }

    /// Load a changeset by its change_id.
    pub fn load_changeset(&self, change_id: &str) -> Result<Changeset> {
        let path = self.vfs_dyna.join("changesets")?.join(&format!("{}.json", change_id))?;
        vfs_exists(&path)?
            .then(|| {
                vfs_read(&path)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
            })
            .unwrap_or_else(|| bail!(DynaError::ChangesetNotFound(change_id.to_string())))
    }

    /// Load all changesets for a channel, in order.
    ///
    /// Uses `filter_map` to skip missing changesets with a warning.
    pub fn load_channel_changesets(&self, channel_name: &str) -> Result<Vec<Changeset>> {
        self.load_channel(channel_name).map(|channel| {
            izip!(&channel.changesets)
                .filter_map(|change_id| {
                    self.load_changeset(change_id)
                        .inspect_err(|e| {
                            eprintln!("Warning: could not load changeset '{}': {}", change_id, e);
                        })
                        .ok()
                })
                .collect_vec()
        })
    }

    /// List all changeset IDs stored locally.
    pub fn all_changeset_ids(&self) -> Result<Vec<String>> {
        collect_vfs_json_stems(&self.vfs_dyna.join("changesets")?)
    }

    /// Find a changeset by prefix of its change_id.
    ///
    /// Filters all IDs by prefix, then loads each match via `try_collect`.
    pub fn find_changeset_by_prefix(&self, prefix: &str) -> Result<Vec<Changeset>> {
        izip!(self.all_changeset_ids()?)
            .filter(|id| id.starts_with(prefix))
            .map(|id| self.load_changeset(&id))
            .try_collect()
    }

    // -----------------------------------------------------------------------
    // Patch Management
    // -----------------------------------------------------------------------

    pub fn store_patch(&self, patch: &Patch) -> Result<()> {
        let hex = dyna_core::hash::strip_prefix(&patch.hash);
        serde_json::to_string_pretty(patch)
            .map_err(Into::into)
            .and_then(|json| {
                vfs_write(
                    &self.vfs_dyna.join("patches")?.join(&format!("{}.json", hex))?,
                    &json,
                )
            })
    }

    pub fn load_patch(&self, hash: &str) -> Result<Patch> {
        let hex = dyna_core::hash::strip_prefix(hash);
        let path = self.vfs_dyna.join("patches")?.join(&format!("{}.json", hex))?;
        vfs_exists(&path)?
            .then(|| {
                vfs_read(&path)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
            })
            .unwrap_or_else(|| bail!(DynaError::PatchNotFound(hash.to_string())))
    }

    // -----------------------------------------------------------------------
    // Staging Area
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
            vfs_read(&path)
                .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
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

    // -----------------------------------------------------------------------
    // Snapshots
    // -----------------------------------------------------------------------

    pub fn save_snapshot(&self, resource_id: &str, value: &Value) -> Result<()> {
        serde_json::to_string_pretty(value)
            .map_err(Into::into)
            .and_then(|json| {
                vfs_write(
                    &self.vfs_dyna.join("snapshots")?.join(&format!("{}.json", resource_id))?,
                    &json,
                )
            })
    }

    pub fn load_snapshot(&self, resource_id: &str) -> Result<Option<Value>> {
        let path = self.vfs_dyna.join("snapshots")?.join(&format!("{}.json", resource_id))?;
        vfs_exists(&path)?
            .then(|| {
                vfs_read(&path)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
                    .map(Some)
            })
            .unwrap_or(Ok(None))
    }

    pub fn load_all_snapshots(&self) -> Result<HashMap<String, Value>> {
        let snapshots_dir = self.vfs_dyna.join("snapshots")?;
        vfs_exists(&snapshots_dir)?
            .then(|| {
                snapshots_dir
                    .read_dir()
                    .map_err(anyhow::Error::from)
                    .and_then(|entries| {
                        entries
                            .filter(|p| p.extension().map_or(false, |ext| ext == "json"))
                            .filter_map(|p| {
                                let fname = p.filename();
                                fname.strip_suffix(".json").map(|stem| {
                                    let key = stem.to_string();
                                    vfs_read(&p)
                                        .and_then(|content| {
                                            serde_json::from_str::<Value>(&content)
                                                .map_err(Into::into)
                                        })
                                        .map(|value| (key, value))
                                })
                            })
                            .try_collect()
                    })
            })
            .unwrap_or_else(|| Ok(HashMap::new()))
    }

    /// Remove a snapshot file.
    pub fn remove_snapshot(&self, resource_id: &str) -> Result<()> {
        let path = self.vfs_dyna.join("snapshots")?.join(&format!("{}.json", resource_id))?;
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

    /// Read a file from the working directory via VFS.
    pub fn read_work_file(&self, relative_path: &str) -> Result<String> {
        vfs_read(&self.vfs_root.join(relative_path)?)
    }

    /// Write a file to the working directory via VFS, creating parent dirs.
    pub fn write_work_file(&self, relative_path: &str, content: &str) -> Result<()> {
        let path = self.vfs_root.join(relative_path)?;
        path.parent().create_dir_all().map_err(anyhow::Error::from)?;
        vfs_write(&path, content)
    }

    /// Check if a file exists in the working directory via VFS.
    pub fn work_file_exists(&self, relative_path: &str) -> Result<bool> {
        vfs_exists(&self.vfs_root.join(relative_path)?)
    }

    /// Remove a file from the working directory via VFS.
    pub fn remove_work_file(&self, relative_path: &str) -> Result<()> {
        let path = self.vfs_root.join(relative_path)?;
        vfs_exists(&path)?
            .then(|| path.remove_file().map_err(Into::into))
            .unwrap_or(Ok(()))
    }

    /// List all `.json` files in the working directory (excluding `.dyna/`),
    /// returning paths relative to the work dir.
    pub fn list_work_json_files(&self) -> Result<Vec<String>> {
        let mut results = Vec::new();
        self.collect_work_json(&self.vfs_root, &mut results)?;
        results.sort();
        Ok(results)
    }

    /// Recursive helper for `list_work_json_files`.
    fn collect_work_json(&self, dir: &VfsPath, results: &mut Vec<String>) -> Result<()> {
        dir.read_dir()
            .map_err(anyhow::Error::from)
            .and_then(|mut entries| {
                entries.try_for_each(|entry| {
                    let fname = entry.filename();
                    // Skip hidden directories (including .dyna)
                    if fname.starts_with('.') {
                        return Ok(());
                    }
                    let is_dir = entry.is_dir().unwrap_or(false);
                    if is_dir {
                        self.collect_work_json(&entry, results)
                    } else if fname.ends_with(".json") {
                        // Compute relative path from vfs_root
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

    /// List all `.json` files under a specific subdirectory in the working
    /// directory (excluding `.dyna/`), returning paths relative to the work dir.
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

    /// Remove a changeset file from `.dyna/changesets/`.
    pub fn remove_changeset_file(&self, change_id: &str) -> Result<()> {
        let path = self.vfs_dyna.join("changesets")?.join(&format!("{}.json", change_id))?;
        vfs_exists(&path)?
            .then(|| path.remove_file().map_err(Into::into))
            .unwrap_or(Ok(()))
    }

    /// Remove a staging file from `.dyna/staging/`.
    pub fn remove_staging_file(&self, resource_id: &str) -> Result<()> {
        let path = self.vfs_dyna.join("staging")?.join(&format!("{}.json", resource_id))?;
        vfs_exists(&path)?
            .then(|| path.remove_file().map_err(Into::into))
            .unwrap_or(Ok(()))
    }

    // -----------------------------------------------------------------------
    // Resource ID extraction
    // -----------------------------------------------------------------------

    pub fn resource_id_from_path(&self, path: &Path) -> String {
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .strip_prefix(&self.work_dir)
            .or_else(|_| path.strip_prefix("."))
            .unwrap_or(path)
            .with_extension("")
            .to_string_lossy()
            .replace('/', ".")
    }

    /// Compute resource ID from a VFS-relative path string.
    pub fn resource_id_from_relative(&self, relative_path: &str) -> String {
        relative_path
            .strip_suffix(".json")
            .unwrap_or(relative_path)
            .replace('/', ".")
    }

    /// Reverse of [`resource_id_from_path`]: converts a dotted resource ID back
    /// into a relative path, appending `.json`.
    ///
    /// For example, `"data.users.config"` becomes `data/users/config.json`.
    ///
    /// This is a pure path computation with **no filesystem side-effects**.
    pub fn path_for_resource_id(&self, resource_id: &str) -> PathBuf {
        let relative = resource_id.replace('.', "/");
        self.work_dir.join(format!("{}.json", relative))
    }

    /// Relative path string for a resource ID (for VFS operations).
    pub fn relative_path_for_resource_id(&self, resource_id: &str) -> String {
        let relative = resource_id.replace('.', "/");
        format!("{}.json", relative)
    }

    /// Like [`path_for_resource_id`], but also creates all intermediate
    /// directories so the file can be written immediately (via VFS).
    pub fn path_from_resource_id(&self, resource_id: &str) -> Result<PathBuf> {
        let relative = self.relative_path_for_resource_id(resource_id);
        let vfs_path = self.vfs_root.join(&relative)?;
        vfs_path
            .parent()
            .create_dir_all()
            .map_err(anyhow::Error::from)?;
        Ok(self.work_dir.join(&relative))
    }

    /// Write a resource file by resource_id, creating parent dirs via VFS.
    pub fn write_resource_file(&self, resource_id: &str, content: &str) -> Result<()> {
        let relative = self.relative_path_for_resource_id(resource_id);
        self.write_work_file(&relative, content)
    }

    /// Read a resource file by resource_id via VFS.
    pub fn read_resource_file(&self, resource_id: &str) -> Result<String> {
        let relative = self.relative_path_for_resource_id(resource_id);
        self.read_work_file(&relative)
    }
}
