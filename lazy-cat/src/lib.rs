//! # lazy-cat
//!
//! A lazy, on-demand resource loader for [Dyna](https://github.com/smunix/dyna)
//! servers.  `LazyClient` maintains an in-memory repository (via `dyna-cli`)
//! that is populated **only** with the resources that have been explicitly
//! requested, while a background WebSocket listener keeps the local cache
//! up-to-date whenever new changesets are pushed to the configured channel.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use lazy_cat::LazyClient;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let client = LazyClient::connect("http://localhost:8080", "main").await?;
//!
//!     // Fetch a single resource on demand
//!     let value = client.get("acme.entity.User").await?;
//!     println!("{}", serde_json::to_string_pretty(&value)?);
//!
//!     // List all available resource IDs (metadata only, no bodies fetched)
//!     let ids = client.list_resources().await?;
//!     println!("Available: {:?}", ids);
//!
//!     Ok(())
//! }
//! ```

mod ws;

use anyhow::{Context, Result};
use dyna_cli::repository::Repository;
use dyna_cli::sync_client::SyncClient;
use dyna_core::notification::{Notification, NotificationKind, NotificationPayload};
use dyna_core::protocol::{CloneRequest, PullRequest};
use itertools::izip;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use vfs::{MemoryFS, VfsPath};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A callback invoked whenever the local cache is updated from the server.
pub type OnUpdateFn = Box<dyn Fn(&[String]) + Send + Sync>;

/// A lazy, on-demand client that loads resources from a remote Dyna server
/// only when they are first requested, then keeps them in sync via WebSocket
/// notifications.
pub struct LazyClient {
    /// The remote server URL.
    server_url: String,
    /// The channel we are tracking.
    channel: String,
    /// HTTP client for pull / clone / history RPCs.
    sync_client: SyncClient,
    /// In-memory dyna-cli repository used as a local cache.
    repo: Arc<RwLock<Repository>>,
    /// Set of resource IDs whose snapshots have been materialised locally.
    loaded: Arc<RwLock<HashSet<String>>>,
    /// Set of *all* known resource IDs on the channel (metadata).
    known_ids: Arc<RwLock<HashSet<String>>>,
    /// Optional user callback fired on every live update.
    on_update: Arc<Mutex<Option<OnUpdateFn>>>,
    /// Handle to the background WebSocket task (kept alive while the client
    /// exists).
    _ws_handle: Option<tokio::task::JoinHandle<()>>,
}

impl LazyClient {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Connect to a Dyna server and start tracking `channel`.
    ///
    /// This performs a lightweight clone of the channel metadata (channel
    /// lineage + changeset list) **without** materialising any resource
    /// snapshots.  Resource bodies are fetched lazily on first access.
    pub async fn connect(server_url: &str, channel: &str) -> Result<Self> {
        let sync_client = SyncClient::new(server_url);

        // Bootstrap an in-memory repository
        let vfs_root: VfsPath = MemoryFS::new().into();
        vfs_root
            .join(".dyna")?
            .create_dir_all()
            .map_err(|e| anyhow::anyhow!("VFS init: {e}"))?;

        // Initialise the repo structure inside the MemoryFS
        let repo = init_memory_repo(vfs_root)?;

        // Clone the channel from the server — we store changesets + channel
        // metadata but do NOT materialise snapshots yet.
        let clone_resp = sync_client
            .clone_repo(&CloneRequest {
                channel: Some(channel.to_string()),
            })
            .await
            .context("Initial clone from server")?;

        // Store changesets
        izip!(&clone_resp.changesets)
            .try_for_each(|cs| repo.store_changeset(cs))?;

        // Store channels
        izip!(&clone_resp.channels)
            .try_for_each(|ch| repo.save_channel(ch))?;

        // Collect the set of all known resource IDs from the changesets
        let known_ids: HashSet<String> = izip!(&clone_resp.changesets)
            .flat_map(|cs| izip!(&cs.patches).map(|p| p.target_resource.clone()))
            .collect();

        // Also include any snapshot keys from the clone response
        let known_ids: HashSet<String> = known_ids
            .into_iter()
            .chain(clone_resp.snapshots.keys().cloned())
            .collect();

        // Pre-store the snapshots that came with the clone response so they
        // are available for lazy materialisation without a second round-trip.
        izip!(&clone_resp.snapshots)
            .try_for_each(|(rid, val)| repo.save_snapshot(rid, val))?;

        let loaded: HashSet<String> = clone_resp.snapshots.keys().cloned().collect();

        // Update sync state
        clone_resp
            .channels
            .iter()
            .find(|ch| ch.name == channel)
            .and_then(|ch| ch.head_change_id.as_ref())
            .map(|head| -> Result<()> {
                let mut ss = repo.load_sync_state()?;
                ss.remote_heads
                    .insert(channel.to_string(), head.clone());
                repo.save_sync_state(&ss)
            })
            .transpose()?;

        let repo = Arc::new(RwLock::new(repo));
        let loaded = Arc::new(RwLock::new(loaded));
        let known_ids = Arc::new(RwLock::new(known_ids));
        let on_update: Arc<Mutex<Option<OnUpdateFn>>> = Arc::new(Mutex::new(None));

        // Spawn the WebSocket listener
        let ws_handle = ws::spawn_listener(
            server_url,
            channel,
            Arc::clone(&repo),
            Arc::clone(&loaded),
            Arc::clone(&known_ids),
            Arc::clone(&on_update),
            sync_client.clone(),
        );

        Ok(Self {
            server_url: server_url.to_string(),
            channel: channel.to_string(),
            sync_client,
            repo,
            loaded,
            known_ids,
            on_update,
            _ws_handle: Some(ws_handle),
        })
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Get a single resource by ID.
    ///
    /// If the resource has not been materialised locally yet, it is fetched
    /// from the server (via a pull of the relevant changeset patches) and
    /// cached for future access.
    pub async fn get(&self, resource_id: &str) -> Result<Value> {
        // Fast path: already loaded
        {
            let loaded = self.loaded.read().await;
            if loaded.contains(resource_id) {
                let repo = self.repo.read().await;
                return repo
                    .load_snapshot(resource_id)?
                    .ok_or_else(|| anyhow::anyhow!("Resource '{}' not found", resource_id));
            }
        }

        // Slow path: materialise from stored changesets
        self.materialise_resource(resource_id).await?;

        let repo = self.repo.read().await;
        repo.load_snapshot(resource_id)?
            .ok_or_else(|| anyhow::anyhow!("Resource '{}' not found after materialisation", resource_id))
    }

    /// Get multiple resources by ID, returning a map of ID → Value.
    ///
    /// Resources not yet loaded are materialised in one pass.
    pub async fn get_many(&self, resource_ids: &[&str]) -> Result<HashMap<String, Value>> {
        // Determine which IDs still need materialisation
        let to_load: Vec<String> = {
            let loaded = self.loaded.read().await;
            izip!(resource_ids)
                .filter(|id| !loaded.contains(**id))
                .map(|id| id.to_string())
                .collect()
        };

        // Materialise missing resources
        for rid in &to_load {
            self.materialise_resource(rid).await?;
        }

        // Read all requested snapshots
        let repo = self.repo.read().await;
        izip!(resource_ids)
            .filter_map(|id| {
                repo.load_snapshot(id)
                    .ok()
                    .flatten()
                    .map(|v| (id.to_string(), v))
            })
            .collect::<HashMap<_, _>>()
            .pipe_ok()
    }

    /// List all known resource IDs on the tracked channel.
    ///
    /// This returns metadata only — no resource bodies are fetched.
    pub async fn list_resources(&self) -> Result<Vec<String>> {
        let ids = self.known_ids.read().await;
        let mut v: Vec<String> = ids.iter().cloned().collect();
        v.sort();
        Ok(v)
    }

    /// Get all resources, materialising any that haven't been loaded yet.
    pub async fn get_all(&self) -> Result<HashMap<String, Value>> {
        let ids: Vec<String> = self.known_ids.read().await.iter().cloned().collect();
        let refs: Vec<&str> = izip!(&ids).map(|s| s.as_str()).collect();
        self.get_many(&refs).await
    }

    /// Return the channel name this client is tracking.
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// Return the server URL.
    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    /// Register a callback that is invoked whenever the local cache is
    /// updated from the server.  The callback receives the list of resource
    /// IDs that were affected.
    pub async fn on_update(&self, f: impl Fn(&[String]) + Send + Sync + 'static) {
        let mut guard = self.on_update.lock().await;
        *guard = Some(Box::new(f));
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Materialise a single resource by replaying its patches from the stored
    /// changesets.
    async fn materialise_resource(&self, resource_id: &str) -> Result<()> {
        let repo = self.repo.write().await;

        // Check if it was loaded while we waited for the write lock
        {
            let loaded = self.loaded.read().await;
            if loaded.contains(resource_id) {
                return Ok(());
            }
        }

        // Try to load from already-stored snapshot first (may have been
        // pre-populated by the clone response)
        if repo.load_snapshot(resource_id)?.is_some() {
            self.loaded.write().await.insert(resource_id.to_string());
            return Ok(());
        }

        // Replay patches from changesets to build the snapshot
        let channel_data = repo.load_channel(&self.channel)?;
        let snapshot = izip!(&channel_data.changesets)
            .filter_map(|cid| repo.load_changeset(cid).ok())
            .flat_map(|cs| cs.patches.into_iter())
            .filter(|p| p.target_resource == resource_id)
            .try_fold(Value::Null, |acc, patch| -> Result<Value> {
                patch
                    .result_snapshot
                    .map(Ok)
                    .unwrap_or_else(|| {
                        let mut current = if acc.is_null() {
                            serde_json::json!({})
                        } else {
                            acc.clone()
                        };
                        dyna_core::diff::apply_patch(&mut current, &patch.operations)
                            .map(|()| current)
                            .map_err(|e| anyhow::anyhow!("apply_patch: {e}"))
                    })
            })?;

        if !snapshot.is_null() {
            repo.save_snapshot(resource_id, &snapshot)?;
            self.loaded.write().await.insert(resource_id.to_string());
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Initialise a `dyna-cli` Repository backed by an in-memory VFS.
fn init_memory_repo(vfs_root: VfsPath) -> Result<Repository> {
    let vfs_dyna = vfs_root.join(".dyna")?;

    // Create subdirectories
    izip!(&[
        "patches",
        "changesets",
        "staging",
        "channels",
        "snapshots",
        "snapshots/main",
        "conflicts",
    ])
    .try_for_each(|sub| -> Result<()> {
        vfs_dyna
            .join(sub)?
            .create_dir_all()
            .map_err(|e| anyhow::anyhow!("mkdir {sub}: {e}"))
    })?;

    // HEAD
    vfs_dyna
        .join("HEAD")?
        .create_file()?
        .write_all(b"main")
        .map_err(|e| anyhow::anyhow!("write HEAD: {e}"))?;

    // WORKING_CHANGE
    vfs_dyna
        .join("WORKING_CHANGE")?
        .create_file()?
        .write_all(b"")
        .map_err(|e| anyhow::anyhow!("write WORKING_CHANGE: {e}"))?;

    // config.toml
    let config = dyna_core::models::RepoConfig::default();
    let config_str = toml::to_string_pretty(&config)?;
    vfs_dyna
        .join("config.toml")?
        .create_file()?
        .write_all(config_str.as_bytes())
        .map_err(|e| anyhow::anyhow!("write config.toml: {e}"))?;

    // Default channel
    let channel = dyna_core::models::Channel::new("main");
    let channel_json = serde_json::to_string_pretty(&channel)?;
    vfs_dyna
        .join("channels")?
        .join("main.json")?
        .create_file()?
        .write_all(channel_json.as_bytes())
        .map_err(|e| anyhow::anyhow!("write main.json: {e}"))?;

    // Sync state
    let ss = dyna_core::models::SyncState::default();
    let ss_json = serde_json::to_string_pretty(&ss)?;
    vfs_dyna
        .join("sync_state.json")?
        .create_file()?
        .write_all(ss_json.as_bytes())
        .map_err(|e| anyhow::anyhow!("write sync_state.json: {e}"))?;

    Ok(Repository::from_vfs(vfs_root))
}

/// Extension trait so we can write `.pipe_ok()` on a value to wrap it in
/// `Ok(...)`.
trait PipeOk: Sized {
    fn pipe_ok(self) -> Result<Self> {
        Ok(self)
    }
}
impl<T> PipeOk for T {}
