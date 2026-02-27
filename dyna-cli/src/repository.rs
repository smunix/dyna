//! Local repository state management.
//!
//! Manages the `.dyna/` directory structure: configuration, channels,
//! changesets, patches, staging area, snapshots, conflicts, and sync state.
//!
//! The storage layout is now changeset-centric:
//!   .dyna/
//!     config.toml
//!     HEAD                  -- current channel name
//!     WORKING_CHANGE        -- change_id of the working-copy changeset
//!     channels/<name>.json  -- channel metadata
//!     changesets/<id>.json  -- changeset objects (keyed by change_id)
//!     patches/<hash>.json   -- individual patch objects
//!     staging/<res>.json    -- staged changes
//!     snapshots/<res>.json  -- resource snapshots
//!     conflicts/<res>.json  -- conflict records
//!     sync_state.json       -- remote sync state

use anyhow::{Context, Result, bail};
use dyna_common::error::DynaError;
use dyna_common::models::*;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const DYNA_DIR: &str = ".dyna";

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
    pub fn find(start_path: &Path) -> Result<Self> {
        let mut current = start_path.to_path_buf();
        loop {
            let dyna_dir = current.join(DYNA_DIR);
            if dyna_dir.is_dir() {
                return Ok(Self {
                    work_dir: current,
                    dyna_dir,
                });
            }
            if !current.pop() {
                bail!(DynaError::NotInitialized);
            }
        }
    }

    /// Find the repository starting from the current working directory.
    pub fn find_current() -> Result<Self> {
        let cwd = std::env::current_dir().context("Failed to get current directory")?;
        Self::find(&cwd)
    }

    /// Initialize a new repository at the given path.
    pub fn init(path: &Path) -> Result<Self> {
        let dyna_dir = path.join(DYNA_DIR);
        if dyna_dir.exists() {
            bail!(DynaError::AlreadyInitialized(path.display().to_string()));
        }

        fs::create_dir_all(dyna_dir.join("patches"))?;
        fs::create_dir_all(dyna_dir.join("changesets"))?;
        fs::create_dir_all(dyna_dir.join("staging"))?;
        fs::create_dir_all(dyna_dir.join("channels"))?;
        fs::create_dir_all(dyna_dir.join("snapshots"))?;
        fs::create_dir_all(dyna_dir.join("conflicts"))?;

        // HEAD points to the current channel
        fs::write(dyna_dir.join("HEAD"), "main")?;

        // No working change yet
        fs::write(dyna_dir.join("WORKING_CHANGE"), "")?;

        // Default config
        let config = RepoConfig::default();
        let config_str = toml::to_string_pretty(&config)?;
        fs::write(dyna_dir.join("config.toml"), config_str)?;

        // Create the default "main" channel
        let main_channel = Channel::new("main");
        let channel_json = serde_json::to_string_pretty(&main_channel)?;
        fs::write(dyna_dir.join("channels/main.json"), channel_json)?;

        // Default sync state
        let sync_state = SyncState::default();
        let sync_json = serde_json::to_string_pretty(&sync_state)?;
        fs::write(dyna_dir.join("sync_state.json"), sync_json)?;

        Ok(Self {
            work_dir: path.to_path_buf(),
            dyna_dir,
        })
    }

    // -----------------------------------------------------------------------
    // Configuration
    // -----------------------------------------------------------------------

    pub fn load_config(&self) -> Result<RepoConfig> {
        let content = fs::read_to_string(self.dyna_dir.join("config.toml"))
            .context("Failed to read config.toml")?;
        let config: RepoConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save_config(&self, config: &RepoConfig) -> Result<()> {
        let config_str = toml::to_string_pretty(config)?;
        fs::write(self.dyna_dir.join("config.toml"), config_str)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // HEAD / Current Channel
    // -----------------------------------------------------------------------

    pub fn current_channel_name(&self) -> Result<String> {
        let head = fs::read_to_string(self.dyna_dir.join("HEAD"))
            .context("Failed to read HEAD")?;
        Ok(head.trim().to_string())
    }

    pub fn set_current_channel(&self, name: &str) -> Result<()> {
        fs::write(self.dyna_dir.join("HEAD"), name)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Working Change (the @ changeset)
    // -----------------------------------------------------------------------

    /// Get the change_id of the current working-copy changeset, if any.
    pub fn working_change_id(&self) -> Result<Option<String>> {
        let path = self.dyna_dir.join("WORKING_CHANGE");
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?.trim().to_string();
        if content.is_empty() {
            Ok(None)
        } else {
            Ok(Some(content))
        }
    }

    /// Set the working-copy changeset.
    pub fn set_working_change(&self, change_id: Option<&str>) -> Result<()> {
        fs::write(
            self.dyna_dir.join("WORKING_CHANGE"),
            change_id.unwrap_or(""),
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Channel Management
    // -----------------------------------------------------------------------

    pub fn load_channel(&self, name: &str) -> Result<Channel> {
        let path = self.dyna_dir.join(format!("channels/{}.json", name));
        if !path.exists() {
            bail!(DynaError::ChannelNotFound(name.to_string()));
        }
        let content = fs::read_to_string(&path)?;
        let channel: Channel = serde_json::from_str(&content)?;
        Ok(channel)
    }

    pub fn save_channel(&self, channel: &Channel) -> Result<()> {
        let path = self.dyna_dir.join(format!("channels/{}.json", channel.name));
        let json = serde_json::to_string_pretty(channel)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn list_channels(&self) -> Result<Vec<Channel>> {
        let channels_dir = self.dyna_dir.join("channels");
        let mut channels = Vec::new();
        for entry in fs::read_dir(&channels_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                let content = fs::read_to_string(&path)?;
                let channel: Channel = serde_json::from_str(&content)?;
                channels.push(channel);
            }
        }
        channels.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(channels)
    }

    pub fn create_channel(&self, name: &str, fork_from: Option<&str>) -> Result<Channel> {
        let path = self.dyna_dir.join(format!("channels/{}.json", name));
        if path.exists() {
            bail!("Channel '{}' already exists", name);
        }

        let channel = if let Some(source_name) = fork_from {
            let source = self.load_channel(source_name)?;
            let mut new_channel = Channel::new(name);
            new_channel.changesets = source.changesets.clone();
            new_channel.head_change_id = source.head_change_id.clone();
            new_channel
        } else {
            Channel::new(name)
        };

        self.save_channel(&channel)?;
        Ok(channel)
    }

    // -----------------------------------------------------------------------
    // Changeset Management
    // -----------------------------------------------------------------------

    /// Store a changeset in the local repository.
    pub fn store_changeset(&self, cs: &Changeset) -> Result<()> {
        let path = self
            .dyna_dir
            .join(format!("changesets/{}.json", cs.change_id));
        let json = serde_json::to_string_pretty(cs)?;
        fs::write(path, json)?;

        // Also store each patch individually for lookup
        for patch in &cs.patches {
            self.store_patch(patch)?;
        }
        Ok(())
    }

    /// Load a changeset by its change_id.
    pub fn load_changeset(&self, change_id: &str) -> Result<Changeset> {
        let path = self
            .dyna_dir
            .join(format!("changesets/{}.json", change_id));
        if !path.exists() {
            bail!(DynaError::ChangesetNotFound(change_id.to_string()));
        }
        let content = fs::read_to_string(&path)?;
        let cs: Changeset = serde_json::from_str(&content)?;
        Ok(cs)
    }

    /// Load all changesets for a channel, in order.
    pub fn load_channel_changesets(&self, channel_name: &str) -> Result<Vec<Changeset>> {
        let channel = self.load_channel(channel_name)?;
        let mut changesets = Vec::new();
        for change_id in &channel.changesets {
            match self.load_changeset(change_id) {
                Ok(cs) => changesets.push(cs),
                Err(e) => {
                    eprintln!(
                        "Warning: could not load changeset '{}': {}",
                        change_id, e
                    );
                }
            }
        }
        Ok(changesets)
    }

    /// List all changeset IDs stored locally.
    pub fn all_changeset_ids(&self) -> Result<Vec<String>> {
        let dir = self.dyna_dir.join("changesets");
        let mut ids = Vec::new();
        if dir.exists() {
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    if let Some(stem) = path.file_stem() {
                        ids.push(stem.to_string_lossy().to_string());
                    }
                }
            }
        }
        Ok(ids)
    }

    /// Find a changeset by prefix of its change_id.
    pub fn find_changeset_by_prefix(&self, prefix: &str) -> Result<Vec<Changeset>> {
        let all_ids = self.all_changeset_ids()?;
        let matches: Vec<String> = all_ids
            .into_iter()
            .filter(|id| id.starts_with(prefix))
            .collect();
        let mut results = Vec::new();
        for id in matches {
            results.push(self.load_changeset(&id)?);
        }
        Ok(results)
    }

    // -----------------------------------------------------------------------
    // Patch Management
    // -----------------------------------------------------------------------

    pub fn store_patch(&self, patch: &Patch) -> Result<()> {
        let hex = dyna_common::hash::strip_prefix(&patch.hash);
        let path = self.dyna_dir.join(format!("patches/{}.json", hex));
        let json = serde_json::to_string_pretty(patch)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_patch(&self, hash: &str) -> Result<Patch> {
        let hex = dyna_common::hash::strip_prefix(hash);
        let path = self.dyna_dir.join(format!("patches/{}.json", hex));
        if !path.exists() {
            bail!(DynaError::PatchNotFound(hash.to_string()));
        }
        let content = fs::read_to_string(&path)?;
        let patch: Patch = serde_json::from_str(&content)?;
        Ok(patch)
    }

    // -----------------------------------------------------------------------
    // Staging Area
    // -----------------------------------------------------------------------

    pub fn stage_change(&self, staged: &StagedChange) -> Result<()> {
        let path = self
            .dyna_dir
            .join(format!("staging/{}.json", staged.resource_id));
        let json = serde_json::to_string_pretty(staged)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_staged_changes(&self) -> Result<Vec<StagedChange>> {
        let staging_dir = self.dyna_dir.join("staging");
        let mut changes = Vec::new();
        if staging_dir.exists() {
            for entry in fs::read_dir(&staging_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    let content = fs::read_to_string(&path)?;
                    let change: StagedChange = serde_json::from_str(&content)?;
                    changes.push(change);
                }
            }
        }
        Ok(changes)
    }

    pub fn clear_staging(&self) -> Result<()> {
        let staging_dir = self.dyna_dir.join("staging");
        if staging_dir.exists() {
            for entry in fs::read_dir(&staging_dir)? {
                let entry = entry?;
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Snapshots
    // -----------------------------------------------------------------------

    pub fn save_snapshot(&self, resource_id: &str, value: &Value) -> Result<()> {
        let path = self
            .dyna_dir
            .join(format!("snapshots/{}.json", resource_id));
        let json = serde_json::to_string_pretty(value)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_snapshot(&self, resource_id: &str) -> Result<Option<Value>> {
        let path = self
            .dyna_dir
            .join(format!("snapshots/{}.json", resource_id));
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        let value: Value = serde_json::from_str(&content)?;
        Ok(Some(value))
    }

    pub fn load_all_snapshots(&self) -> Result<HashMap<String, Value>> {
        let snapshots_dir = self.dyna_dir.join("snapshots");
        let mut snapshots = HashMap::new();
        if snapshots_dir.exists() {
            for entry in fs::read_dir(&snapshots_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    if let Some(stem) = path.file_stem() {
                        let content = fs::read_to_string(&path)?;
                        let value: Value = serde_json::from_str(&content)?;
                        snapshots.insert(stem.to_string_lossy().to_string(), value);
                    }
                }
            }
        }
        Ok(snapshots)
    }

    /// Remove a snapshot file.
    pub fn remove_snapshot(&self, resource_id: &str) -> Result<()> {
        let path = self
            .dyna_dir
            .join(format!("snapshots/{}.json", resource_id));
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Sync State
    // -----------------------------------------------------------------------

    pub fn load_sync_state(&self) -> Result<SyncState> {
        let path = self.dyna_dir.join("sync_state.json");
        if !path.exists() {
            return Ok(SyncState::default());
        }
        let content = fs::read_to_string(&path)?;
        let state: SyncState = serde_json::from_str(&content)?;
        Ok(state)
    }

    pub fn save_sync_state(&self, state: &SyncState) -> Result<()> {
        let json = serde_json::to_string_pretty(state)?;
        fs::write(self.dyna_dir.join("sync_state.json"), json)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Conflicts
    // -----------------------------------------------------------------------

    pub fn save_conflicts(
        &self,
        resource_id: &str,
        conflicts: &[Conflict],
    ) -> Result<()> {
        let path = self
            .dyna_dir
            .join(format!("conflicts/{}.json", resource_id));
        let json = serde_json::to_string_pretty(conflicts)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_conflicts(&self, resource_id: &str) -> Result<Vec<Conflict>> {
        let path = self
            .dyna_dir
            .join(format!("conflicts/{}.json", resource_id));
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path)?;
        let conflicts: Vec<Conflict> = serde_json::from_str(&content)?;
        Ok(conflicts)
    }

    pub fn list_conflicted_resources(&self) -> Result<Vec<String>> {
        let conflicts_dir = self.dyna_dir.join("conflicts");
        let mut resources = Vec::new();
        if conflicts_dir.exists() {
            for entry in fs::read_dir(&conflicts_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    if let Some(stem) = path.file_stem() {
                        resources.push(stem.to_string_lossy().to_string());
                    }
                }
            }
        }
        Ok(resources)
    }

    pub fn clear_conflicts(&self, resource_id: &str) -> Result<()> {
        let path = self
            .dyna_dir
            .join(format!("conflicts/{}.json", resource_id));
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Resource ID extraction
    // -----------------------------------------------------------------------

    pub fn resource_id_from_path(path: &Path) -> String {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string())
    }
}
