//! Minimal in-memory repository for lazy-wasm.
//!
//! Stores changesets, channels, snapshots, and sync state in a `MemoryFS`
//! backed VFS.  Only the subset of `dyna-cli/repository.rs` needed for
//! lazy loading is implemented.

use anyhow::{Context, Result};
use dyna_core::compression;
use dyna_core::models::*;
use itertools::izip;
use serde_json::Value;
use std::io::Write;
use vfs::{MemoryFS, VfsPath};

const DYNA_DIR: &str = ".dyna";

// ---------------------------------------------------------------------------
// VFS helpers
// ---------------------------------------------------------------------------

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

fn vfs_read(path: &VfsPath) -> Result<String> {
    let mut buf = Vec::new();
    path.open_file()
        .map_err(anyhow::Error::from)
        .and_then(|mut f| {
            std::io::Read::read_to_end(&mut f, &mut buf)?;
            Ok(())
        })
        .and_then(|()| {
            compression::read_transparent_str(&buf)
                .map_err(|e| anyhow::anyhow!("decompress: {e}"))
        })
}

pub struct Repository {
    root: VfsPath,
}

impl Repository {
    pub fn new() -> Self {
        let root: VfsPath = MemoryFS::new().into();
        Self { root }
    }

    pub fn init(&self) -> Result<()> {
        let dyna = self.root.join(DYNA_DIR)?;
        izip!(&[
            "changesets",
            "channels",
            "snapshots",
            "snapshots/main",
        ])
        .try_for_each(|sub| -> Result<()> {
            dyna.join(sub)?
                .create_dir_all()
                .map_err(|e| anyhow::anyhow!("mkdir {sub}: {e}"))
        })?;

        // HEAD
        dyna.join("HEAD")?
            .create_file()?
            .write_all(b"main")
            .map_err(|e| anyhow::anyhow!("write HEAD: {e}"))?;

        // Default channel
        let channel = Channel::new("main");
        let channel_json = serde_json::to_string_pretty(&channel)?;
        vfs_write(&dyna.join("channels/main.json")?, &channel_json)?;

        // Sync state
        let ss = SyncState::default();
        let ss_json = serde_json::to_string_pretty(&ss)?;
        vfs_write(&dyna.join("sync_state.json")?, &ss_json)?;

        // Config
        let config = RepoConfig::default();
        let config_str = toml::to_string_pretty(&config)?;
        vfs_write(&dyna.join("config.toml")?, &config_str)?;

        Ok(())
    }

    fn dyna_path(&self) -> Result<VfsPath> {
        self.root.join(DYNA_DIR).map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Changesets
    // -----------------------------------------------------------------------

    pub fn store_changeset(&self, cs: &Changeset) -> Result<()> {
        let path = self
            .dyna_path()?
            .join(&format!("changesets/{}.json", cs.change_id))?;
        let json = serde_json::to_string_pretty(cs)?;
        vfs_write(&path, &json)
    }

    pub fn load_changeset(&self, change_id: &str) -> Result<Changeset> {
        let path = self
            .dyna_path()?
            .join(&format!("changesets/{change_id}.json"))?;
        let content = vfs_read(&path).context(format!("load changeset {change_id}"))?;
        serde_json::from_str(&content).map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Channels
    // -----------------------------------------------------------------------

    pub fn save_channel(&self, channel: &Channel) -> Result<()> {
        let path = self
            .dyna_path()?
            .join(&format!("channels/{}.json", channel.name))?;
        let json = serde_json::to_string_pretty(channel)?;
        vfs_write(&path, &json)
    }

    pub fn load_channel(&self, name: &str) -> Result<Channel> {
        let path = self.dyna_path()?.join(&format!("channels/{name}.json"))?;
        let content = vfs_read(&path).context(format!("load channel {name}"))?;
        serde_json::from_str(&content).map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Snapshots
    // -----------------------------------------------------------------------

    pub fn save_snapshot(&self, resource_id: &str, value: &Value) -> Result<()> {
        let channel = self.current_channel()?;
        let path = self
            .dyna_path()?
            .join(&format!("snapshots/{channel}/{resource_id}.json"))?;
        let json = serde_json::to_string_pretty(value)?;
        vfs_write(&path, &json)
    }

    pub fn load_snapshot(&self, resource_id: &str) -> Result<Option<Value>> {
        let channel = self.current_channel()?;
        let path = self
            .dyna_path()?
            .join(&format!("snapshots/{channel}/{resource_id}.json"))?;
        path.exists()
            .map_err(anyhow::Error::from)
            .and_then(|exists| {
                exists
                    .then(|| {
                        vfs_read(&path)
                            .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
                            .map(Some)
                    })
                    .unwrap_or(Ok(None))
            })
    }

    pub fn list_snapshot_ids(&self) -> Result<Vec<String>> {
        let channel = self.current_channel()?;
        let dir = self
            .dyna_path()?
            .join(&format!("snapshots/{channel}"))?;
        dir.exists()
            .map_err(anyhow::Error::from)
            .and_then(|exists| {
                exists
                    .then(|| {
                        dir.read_dir()
                            .map_err(anyhow::Error::from)
                            .map(|entries| {
                                entries
                                    .filter_map(|p| {
                                        let fname = p.filename();
                                        fname.strip_suffix(".json").map(|s| s.to_string())
                                    })
                                    .collect()
                            })
                    })
                    .unwrap_or_else(|| Ok(Vec::new()))
            })
    }

    // -----------------------------------------------------------------------
    // Sync state
    // -----------------------------------------------------------------------

    pub fn load_sync_state(&self) -> Result<SyncState> {
        let path = self.dyna_path()?.join("sync_state.json")?;
        path.exists()
            .map_err(anyhow::Error::from)
            .and_then(|exists| {
                exists
                    .then(|| {
                        vfs_read(&path)
                            .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
                    })
                    .unwrap_or_else(|| Ok(SyncState::default()))
            })
    }

    pub fn save_sync_state(&self, ss: &SyncState) -> Result<()> {
        let path = self.dyna_path()?.join("sync_state.json")?;
        let json = serde_json::to_string_pretty(ss)?;
        vfs_write(&path, &json)
    }

    // -----------------------------------------------------------------------
    // Current channel
    // -----------------------------------------------------------------------

    pub fn current_channel(&self) -> Result<String> {
        let path = self.dyna_path()?.join("HEAD")?;
        vfs_read(&path)
            .map(|s| s.trim().to_string())
            .or_else(|_| Ok("main".to_string()))
    }

    pub fn switch_channel(&self, name: &str) -> Result<()> {
        let path = self.dyna_path()?.join("HEAD")?;
        vfs_write_bytes(&path, name.as_bytes())?;
        // Ensure snapshots directory exists for the channel
        self.dyna_path()?
            .join(&format!("snapshots/{name}"))?
            .create_dir_all()
            .map_err(|e| anyhow::anyhow!("mkdir snapshots/{name}: {e}"))?;
        Ok(())
    }
}
