//! Local repository state management.
//!
//! Manages the `.dyna/` directory structure, configuration, channels, patches,
//! staging area, and sync state.

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
            bail!(DynaError::AlreadyInitialized(
                path.display().to_string()
            ));
        }

        // Create directory structure
        fs::create_dir_all(dyna_dir.join("patches"))?;
        fs::create_dir_all(dyna_dir.join("staging"))?;
        fs::create_dir_all(dyna_dir.join("channels"))?;
        fs::create_dir_all(dyna_dir.join("snapshots"))?;
        fs::create_dir_all(dyna_dir.join("conflicts"))?;

        // Write default HEAD
        fs::write(dyna_dir.join("HEAD"), "main")?;

        // Write default config
        let config = RepoConfig::default();
        let config_str = toml::to_string_pretty(&config)?;
        fs::write(dyna_dir.join("config.toml"), config_str)?;

        // Create the default "main" channel
        let main_channel = Channel::new("main");
        let channel_json = serde_json::to_string_pretty(&main_channel)?;
        fs::write(dyna_dir.join("channels/main.json"), channel_json)?;

        // Write default sync state
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

    /// Load the repository configuration.
    pub fn load_config(&self) -> Result<RepoConfig> {
        let config_path = self.dyna_dir.join("config.toml");
        let content = fs::read_to_string(&config_path)
            .context("Failed to read config.toml")?;
        let config: RepoConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save the repository configuration.
    pub fn save_config(&self, config: &RepoConfig) -> Result<()> {
        let config_str = toml::to_string_pretty(config)?;
        fs::write(self.dyna_dir.join("config.toml"), config_str)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // HEAD / Current Channel
    // -----------------------------------------------------------------------

    /// Get the name of the current channel.
    pub fn current_channel_name(&self) -> Result<String> {
        let head = fs::read_to_string(self.dyna_dir.join("HEAD"))
            .context("Failed to read HEAD")?;
        Ok(head.trim().to_string())
    }

    /// Set the current channel.
    pub fn set_current_channel(&self, name: &str) -> Result<()> {
        fs::write(self.dyna_dir.join("HEAD"), name)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Channel Management
    // -----------------------------------------------------------------------

    /// Load a channel by name.
    pub fn load_channel(&self, name: &str) -> Result<Channel> {
        let path = self.dyna_dir.join(format!("channels/{}.json", name));
        if !path.exists() {
            bail!(DynaError::ChannelNotFound(name.to_string()));
        }
        let content = fs::read_to_string(&path)?;
        let channel: Channel = serde_json::from_str(&content)?;
        Ok(channel)
    }

    /// Load the current channel.
    pub fn load_current_channel(&self) -> Result<Channel> {
        let name = self.current_channel_name()?;
        self.load_channel(&name)
    }

    /// Save a channel.
    pub fn save_channel(&self, channel: &Channel) -> Result<()> {
        let path = self.dyna_dir.join(format!("channels/{}.json", channel.name));
        let json = serde_json::to_string_pretty(channel)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// List all channels.
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

    /// Create a new channel, optionally forking from an existing one.
    pub fn create_channel(&self, name: &str, fork_from: Option<&str>) -> Result<Channel> {
        let path = self.dyna_dir.join(format!("channels/{}.json", name));
        if path.exists() {
            bail!("Channel '{}' already exists", name);
        }

        let channel = if let Some(source_name) = fork_from {
            let source = self.load_channel(source_name)?;
            let mut new_channel = Channel::new(name);
            new_channel.patches = source.patches.clone();
            new_channel.head = source.head.clone();
            new_channel
        } else {
            Channel::new(name)
        };

        self.save_channel(&channel)?;
        Ok(channel)
    }

    // -----------------------------------------------------------------------
    // Patch Management
    // -----------------------------------------------------------------------

    /// Store a patch in the local repository.
    pub fn store_patch(&self, patch: &Patch) -> Result<()> {
        let hex = dyna_common::hash::strip_prefix(&patch.hash);
        let path = self.dyna_dir.join(format!("patches/{}.json", hex));
        let json = serde_json::to_string_pretty(patch)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load a patch by its hash.
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

    /// Load all patches for the current channel, in order.
    pub fn load_channel_patches(&self, channel_name: &str) -> Result<Vec<Patch>> {
        let channel = self.load_channel(channel_name)?;
        let mut patches = Vec::new();
        for hash in &channel.patches {
            patches.push(self.load_patch(hash)?);
        }
        Ok(patches)
    }

    /// Get all known patch hashes.
    pub fn all_patch_hashes(&self) -> Result<Vec<String>> {
        let patches_dir = self.dyna_dir.join("patches");
        let mut hashes = Vec::new();
        if patches_dir.exists() {
            for entry in fs::read_dir(&patches_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    if let Some(stem) = path.file_stem() {
                        hashes.push(format!("sha256:{}", stem.to_string_lossy()));
                    }
                }
            }
        }
        Ok(hashes)
    }

    // -----------------------------------------------------------------------
    // Staging Area
    // -----------------------------------------------------------------------

    /// Stage a change for a resource.
    pub fn stage_change(&self, staged: &StagedChange) -> Result<()> {
        let path = self.dyna_dir.join(format!("staging/{}.json", staged.resource_id));
        let json = serde_json::to_string_pretty(staged)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load all staged changes.
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

    /// Clear the staging area.
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

    /// Save a resource snapshot.
    pub fn save_snapshot(&self, resource_id: &str, value: &Value) -> Result<()> {
        let path = self.dyna_dir.join(format!("snapshots/{}.json", resource_id));
        let json = serde_json::to_string_pretty(value)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load a resource snapshot.
    pub fn load_snapshot(&self, resource_id: &str) -> Result<Option<Value>> {
        let path = self.dyna_dir.join(format!("snapshots/{}.json", resource_id));
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        let value: Value = serde_json::from_str(&content)?;
        Ok(Some(value))
    }

    /// Load all snapshots.
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

    // -----------------------------------------------------------------------
    // Sync State
    // -----------------------------------------------------------------------

    /// Load the sync state.
    pub fn load_sync_state(&self) -> Result<SyncState> {
        let path = self.dyna_dir.join("sync_state.json");
        if !path.exists() {
            return Ok(SyncState::default());
        }
        let content = fs::read_to_string(&path)?;
        let state: SyncState = serde_json::from_str(&content)?;
        Ok(state)
    }

    /// Save the sync state.
    pub fn save_sync_state(&self, state: &SyncState) -> Result<()> {
        let json = serde_json::to_string_pretty(state)?;
        fs::write(self.dyna_dir.join("sync_state.json"), json)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Conflicts
    // -----------------------------------------------------------------------

    /// Save conflicts for a resource.
    pub fn save_conflicts(&self, resource_id: &str, conflicts: &[dyna_common::models::Conflict]) -> Result<()> {
        let path = self.dyna_dir.join(format!("conflicts/{}.json", resource_id));
        let json = serde_json::to_string_pretty(conflicts)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load conflicts for a resource.
    pub fn load_conflicts(&self, resource_id: &str) -> Result<Vec<dyna_common::models::Conflict>> {
        let path = self.dyna_dir.join(format!("conflicts/{}.json", resource_id));
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path)?;
        let conflicts: Vec<dyna_common::models::Conflict> = serde_json::from_str(&content)?;
        Ok(conflicts)
    }

    /// List all resources with unresolved conflicts.
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

    /// Clear conflicts for a resource.
    pub fn clear_conflicts(&self, resource_id: &str) -> Result<()> {
        let path = self.dyna_dir.join(format!("conflicts/{}.json", resource_id));
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Resource ID extraction
    // -----------------------------------------------------------------------

    /// Derive a resource ID from a file path.
    ///
    /// The resource ID is the file stem (name without extension).
    pub fn resource_id_from_path(path: &Path) -> String {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string())
    }
}
