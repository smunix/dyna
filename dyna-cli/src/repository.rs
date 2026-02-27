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
use std::fs;
use std::path::{Path, PathBuf};

const DYNA_DIR: &str = ".dyna";

/// Helper to read a directory, filter JSON files, and collect results via a
/// mapping function. Uses iterator chains with `try_fold` semantics.
fn collect_json_entries<T, F>(dir: &Path, map_fn: F) -> Result<Vec<T>>
where
    F: Fn(PathBuf) -> Result<T>,
{
    dir.exists()
        .then(|| {
            fs::read_dir(dir)?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| path.extension().map_or(false, |ext| ext == "json"))
                .map(map_fn)
                .try_collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

/// Helper to read JSON file stems from a directory.
fn collect_json_stems(dir: &Path) -> Result<Vec<String>> {
    collect_json_entries(dir, |path| {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .ok_or_else(|| anyhow::anyhow!("Invalid file stem"))
    })
}

/// Represents a local Dyna repository.
pub struct Repository {
    /// The working directory (project root).
    pub work_dir: PathBuf,
    /// The `.dyna` metadata directory.
    pub dyna_dir: PathBuf,
}

impl Repository {
    // -----------------------------------------------------------------------
    // Discovery & Initialization
    // -----------------------------------------------------------------------

    /// Find an existing repository by walking up from `start_path`.
    ///
    /// Uses `std::iter::successors` to generate parent paths, then finds the
    /// first one containing a `.dyna` directory.
    pub fn find(start_path: &Path) -> Result<Self> {
        std::iter::successors(Some(start_path.to_path_buf()), |current| {
            current.parent().map(|p| p.to_path_buf())
        })
        .find(|current| current.join(DYNA_DIR).is_dir())
        .map(|current| Self {
            dyna_dir: current.join(DYNA_DIR),
            work_dir: current,
        })
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
        let dyna_dir = path.join(DYNA_DIR);
        if dyna_dir.exists() {
            bail!(DynaError::AlreadyInitialized(path.display().to_string()));
        }

        // Create all subdirectories via iterator
        izip!(&["patches", "changesets", "staging", "channels", "snapshots", "conflicts"])
            .try_for_each(|sub| fs::create_dir_all(dyna_dir.join(sub)).map_err(anyhow::Error::from))?;

        // HEAD points to the current channel
        fs::write(dyna_dir.join("HEAD"), "main")?;

        // No working change yet
        fs::write(dyna_dir.join("WORKING_CHANGE"), "")?;

        // Default config
        toml::to_string_pretty(&RepoConfig::default())
            .map_err(anyhow::Error::from)
            .and_then(|config_str| fs::write(dyna_dir.join("config.toml"), config_str).map_err(anyhow::Error::from))?;

        // Create the default "main" channel
        serde_json::to_string_pretty(&Channel::new("main"))
            .map_err(anyhow::Error::from)
            .and_then(|json| fs::write(dyna_dir.join("channels/main.json"), json).map_err(anyhow::Error::from))?;

        // Default sync state
        serde_json::to_string_pretty(&SyncState::default())
            .map_err(anyhow::Error::from)
            .and_then(|json| fs::write(dyna_dir.join("sync_state.json"), json).map_err(anyhow::Error::from))?;

        Ok(Self {
            work_dir: path.to_path_buf(),
            dyna_dir,
        })
    }

    // -----------------------------------------------------------------------
    // Configuration
    // -----------------------------------------------------------------------

    pub fn load_config(&self) -> Result<RepoConfig> {
        fs::read_to_string(self.dyna_dir.join("config.toml"))
            .context("Failed to read config.toml")
            .and_then(|content| toml::from_str(&content).map_err(Into::into))
    }

    pub fn save_config(&self, config: &RepoConfig) -> Result<()> {
        toml::to_string_pretty(config)
            .map_err(Into::into)
            .and_then(|s| fs::write(self.dyna_dir.join("config.toml"), s).map_err(Into::into))
    }

    // -----------------------------------------------------------------------
    // HEAD / Current Channel
    // -----------------------------------------------------------------------

    pub fn current_channel_name(&self) -> Result<String> {
        fs::read_to_string(self.dyna_dir.join("HEAD"))
            .context("Failed to read HEAD")
            .map(|head| head.trim().to_string())
    }

    pub fn set_current_channel(&self, name: &str) -> Result<()> {
        fs::write(self.dyna_dir.join("HEAD"), name).map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Working Change (the @ changeset)
    // -----------------------------------------------------------------------

    /// Get the change_id of the current working-copy changeset, if any.
    pub fn working_change_id(&self) -> Result<Option<String>> {
        let path = self.dyna_dir.join("WORKING_CHANGE");
        path.exists()
            .then(|| {
                fs::read_to_string(&path)
                    .map(|s| s.trim().to_string())
                    .map(|s| (!s.is_empty()).then_some(s))
                    .map_err(Into::into)
            })
            .unwrap_or(Ok(None))
    }

    /// Set the working-copy changeset.
    pub fn set_working_change(&self, change_id: Option<&str>) -> Result<()> {
        fs::write(
            self.dyna_dir.join("WORKING_CHANGE"),
            change_id.unwrap_or(""),
        )
        .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Channel Management
    // -----------------------------------------------------------------------

    pub fn load_channel(&self, name: &str) -> Result<Channel> {
        let path = self.dyna_dir.join(format!("channels/{}.json", name));
        path.exists()
            .then(|| {
                fs::read_to_string(&path)
                    .map_err(Into::into)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
            })
            .unwrap_or_else(|| bail!(DynaError::ChannelNotFound(name.to_string())))
    }

    pub fn save_channel(&self, channel: &Channel) -> Result<()> {
        serde_json::to_string_pretty(channel)
            .map_err(Into::into)
            .and_then(|json| {
                fs::write(
                    self.dyna_dir.join(format!("channels/{}.json", channel.name)),
                    json,
                )
                .map_err(Into::into)
            })
    }

    pub fn list_channels(&self) -> Result<Vec<Channel>> {
        collect_json_entries(&self.dyna_dir.join("channels"), |path| {
            fs::read_to_string(&path)
                .map_err(Into::into)
                .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
        })
        .map(|mut channels: Vec<Channel>| {
            channels.sort_by(|a, b| a.name.cmp(&b.name));
            channels
        })
    }

    pub fn create_channel(&self, name: &str, fork_from: Option<&str>) -> Result<Channel> {
        let path = self.dyna_dir.join(format!("channels/{}.json", name));
        if path.exists() {
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
                fs::write(
                    self.dyna_dir.join(format!("changesets/{}.json", cs.change_id)),
                    json,
                )
                .map_err(Into::into)
            })
            .and_then(|()| {
                izip!(&cs.patches).try_for_each(|patch| self.store_patch(patch))
            })
    }

    /// Load a changeset by its change_id.
    pub fn load_changeset(&self, change_id: &str) -> Result<Changeset> {
        let path = self.dyna_dir.join(format!("changesets/{}.json", change_id));
        path.exists()
            .then(|| {
                fs::read_to_string(&path)
                    .map_err(Into::into)
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
        collect_json_stems(&self.dyna_dir.join("changesets"))
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
                fs::write(self.dyna_dir.join(format!("patches/{}.json", hex)), json)
                    .map_err(Into::into)
            })
    }

    pub fn load_patch(&self, hash: &str) -> Result<Patch> {
        let hex = dyna_core::hash::strip_prefix(hash);
        let path = self.dyna_dir.join(format!("patches/{}.json", hex));
        path.exists()
            .then(|| {
                fs::read_to_string(&path)
                    .map_err(Into::into)
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
                fs::write(
                    self.dyna_dir.join(format!("staging/{}.json", staged.resource_id)),
                    json,
                )
                .map_err(Into::into)
            })
    }

    pub fn load_staged_changes(&self) -> Result<Vec<StagedChange>> {
        collect_json_entries(&self.dyna_dir.join("staging"), |path| {
            fs::read_to_string(&path)
                .map_err(Into::into)
                .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
        })
    }

    pub fn clear_staging(&self) -> Result<()> {
        let staging_dir = self.dyna_dir.join("staging");
        staging_dir
            .exists()
            .then(|| {
                fs::read_dir(&staging_dir)?
                    .filter_map(|entry| entry.ok())
                    .try_for_each(|entry| fs::remove_file(entry.path()).map_err(Into::into))
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
                fs::write(
                    self.dyna_dir.join(format!("snapshots/{}.json", resource_id)),
                    json,
                )
                .map_err(Into::into)
            })
    }

    pub fn load_snapshot(&self, resource_id: &str) -> Result<Option<Value>> {
        let path = self.dyna_dir.join(format!("snapshots/{}.json", resource_id));
        path.exists()
            .then(|| {
                fs::read_to_string(&path)
                    .map_err(Into::into)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
                    .map(Some)
            })
            .unwrap_or(Ok(None))
    }

    pub fn load_all_snapshots(&self) -> Result<HashMap<String, Value>> {
        let snapshots_dir = self.dyna_dir.join("snapshots");
        snapshots_dir
            .exists()
            .then(|| {
                fs::read_dir(&snapshots_dir)?
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().map_or(false, |ext| ext == "json"))
                    .filter_map(|path| {
                        path.file_stem().map(|stem| {
                            let key = stem.to_string_lossy().to_string();
                            fs::read_to_string(&path)
                                .map_err(Into::into)
                                .and_then(|content| {
                                    serde_json::from_str::<Value>(&content).map_err(Into::into)
                                })
                                .map(|value| (key, value))
                        })
                    })
                    .try_collect()
            })
            .unwrap_or_else(|| Ok(HashMap::new()))
    }

    /// Remove a snapshot file.
    pub fn remove_snapshot(&self, resource_id: &str) -> Result<()> {
        let path = self.dyna_dir.join(format!("snapshots/{}.json", resource_id));
        path.exists()
            .then(|| fs::remove_file(path).map_err(Into::into))
            .unwrap_or(Ok(()))
    }

    // -----------------------------------------------------------------------
    // Sync State
    // -----------------------------------------------------------------------

    pub fn load_sync_state(&self) -> Result<SyncState> {
        let path = self.dyna_dir.join("sync_state.json");
        path.exists()
            .then(|| {
                fs::read_to_string(&path)
                    .map_err(Into::into)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
            })
            .unwrap_or_else(|| Ok(SyncState::default()))
    }

    pub fn save_sync_state(&self, state: &SyncState) -> Result<()> {
        serde_json::to_string_pretty(state)
            .map_err(Into::into)
            .and_then(|json| {
                fs::write(self.dyna_dir.join("sync_state.json"), json).map_err(Into::into)
            })
    }

    // -----------------------------------------------------------------------
    // Conflicts
    // -----------------------------------------------------------------------

    pub fn save_conflicts(&self, resource_id: &str, conflicts: &[Conflict]) -> Result<()> {
        serde_json::to_string_pretty(conflicts)
            .map_err(Into::into)
            .and_then(|json| {
                fs::write(
                    self.dyna_dir.join(format!("conflicts/{}.json", resource_id)),
                    json,
                )
                .map_err(Into::into)
            })
    }

    pub fn load_conflicts(&self, resource_id: &str) -> Result<Vec<Conflict>> {
        let path = self.dyna_dir.join(format!("conflicts/{}.json", resource_id));
        path.exists()
            .then(|| {
                fs::read_to_string(&path)
                    .map_err(Into::into)
                    .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
            })
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    pub fn list_conflicted_resources(&self) -> Result<Vec<String>> {
        collect_json_stems(&self.dyna_dir.join("conflicts"))
    }

    pub fn clear_conflicts(&self, resource_id: &str) -> Result<()> {
        let path = self.dyna_dir.join(format!("conflicts/{}.json", resource_id));
        path.exists()
            .then(|| fs::remove_file(path).map_err(Into::into))
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
}
