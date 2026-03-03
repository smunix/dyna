//! Background WebSocket listener that keeps the local cache in sync.
//!
//! Connects to `GET /api/v1/ws` on the Dyna server and listens for
//! [`Notification`] events.  When a push or promotion targets the tracked
//! channel, the listener automatically pulls the new changesets and updates
//! the in-memory repository.

use crate::OnUpdateFn;
use anyhow::Result;
use dyna_cli::repository::Repository;
use dyna_cli::sync_client::SyncClient;
use dyna_core::notification::{Notification, NotificationKind, NotificationPayload};
use dyna_core::protocol::PullRequest;
use futures::StreamExt;
use itertools::izip;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::connect_async;

/// Spawn a background task that listens on the server's WebSocket endpoint
/// and pulls new changesets whenever the tracked channel is updated.
pub fn spawn_listener(
    server_url: &str,
    channel: &str,
    repo: Arc<RwLock<Repository>>,
    loaded: Arc<RwLock<HashSet<String>>>,
    known_ids: Arc<RwLock<HashSet<String>>>,
    on_update: Arc<Mutex<Option<OnUpdateFn>>>,
    sync_client: SyncClient,
) -> tokio::task::JoinHandle<()> {
    let ws_url = server_url
        .replace("http://", "ws://")
        .replace("https://", "wss://")
        + "/api/v1/ws";
    let channel = channel.to_string();

    tokio::spawn(async move {
        loop {
            match try_listen(
                &ws_url,
                &channel,
                &repo,
                &loaded,
                &known_ids,
                &on_update,
                &sync_client,
            )
            .await
            {
                Ok(()) => {
                    tracing::info!("WebSocket connection closed, reconnecting in 5s…");
                }
                Err(e) => {
                    tracing::warn!("WebSocket error: {e}, reconnecting in 5s…");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    })
}

/// One connection attempt.  Returns when the connection drops.
async fn try_listen(
    ws_url: &str,
    channel: &str,
    repo: &Arc<RwLock<Repository>>,
    loaded: &Arc<RwLock<HashSet<String>>>,
    known_ids: &Arc<RwLock<HashSet<String>>>,
    on_update: &Arc<Mutex<Option<OnUpdateFn>>>,
    sync_client: &SyncClient,
) -> Result<()> {
    let (ws_stream, _) = connect_async(ws_url).await?;
    let (_write, mut read) = ws_stream.split();

    tracing::info!("WebSocket connected to {ws_url}");

    while let Some(msg) = read.next().await {
        let text = msg?
            .into_text()
            .map_err(|e| anyhow::anyhow!("ws text: {e}"))?;

        // Parse the notification
        let notification = match Notification::from_json(&text) {
            Ok(n) => n,
            Err(e) => {
                tracing::debug!("Ignoring unparseable WS message: {e}");
                continue;
            }
        };

        // Determine which channel was affected and which resources changed
        let (affected_channel, affected_resources) = match &notification.payload {
            NotificationPayload::Push(p) => (
                p.channel.clone(),
                izip!(&p.changesets)
                    .flat_map(|cs| izip!(&cs.affected_resources).cloned())
                    .collect::<Vec<_>>(),
            ),
            NotificationPayload::Promotion(p) => (
                p.target_channel.clone(),
                izip!(&p.promoted_changesets)
                    .flat_map(|cs| izip!(&cs.affected_resources).cloned())
                    .collect::<Vec<_>>(),
            ),
        };

        // Only process events for our tracked channel
        if affected_channel != channel {
            continue;
        }

        tracing::info!(
            "Received {:?} notification for channel '{}', {} resource(s) affected",
            notification.kind,
            channel,
            affected_resources.len()
        );

        // Pull the latest changesets
        if let Err(e) = pull_updates(
            channel,
            repo,
            loaded,
            known_ids,
            sync_client,
        )
        .await
        {
            tracing::error!("Failed to pull updates: {e}");
            continue;
        }

        // Fire the user callback
        let guard = on_update.lock().await;
        guard
            .as_ref()
            .map(|f| f(&affected_resources));
    }

    Ok(())
}

/// Pull new changesets from the server and update the local cache.
async fn pull_updates(
    channel: &str,
    repo: &Arc<RwLock<Repository>>,
    loaded: &Arc<RwLock<HashSet<String>>>,
    known_ids: &Arc<RwLock<HashSet<String>>>,
    sync_client: &SyncClient,
) -> Result<()> {
    let since = {
        let r = repo.read().await;
        r.load_sync_state()?
            .remote_heads
            .get(channel)
            .cloned()
    };

    let response = sync_client
        .pull(&PullRequest {
            channel: channel.to_string(),
            since_change_id: since,
        })
        .await?;

    if response.changesets.is_empty() {
        return Ok(());
    }

    let mut repo = repo.write().await;
    let mut loaded = loaded.write().await;
    let mut known = known_ids.write().await;

    // Store new changesets and update snapshots for already-loaded resources
    izip!(&response.changesets).try_for_each(|cs| -> Result<()> {
        repo.store_changeset(cs)?;

        izip!(&cs.patches).for_each(|patch| {
            // Track the resource as known
            known.insert(patch.target_resource.clone());

            // If we already have this resource loaded, update its snapshot
            if loaded.contains(&patch.target_resource) {
                patch
                    .result_snapshot
                    .as_ref()
                    .map(|snap| {
                        let _ = repo.save_snapshot(&patch.target_resource, snap);
                    })
                    .unwrap_or_else(|| {
                        // Apply patch operations to existing snapshot
                        repo.load_snapshot(&patch.target_resource)
                            .ok()
                            .flatten()
                            .map(|mut current| {
                                dyna_core::diff::apply_patch(&mut current, &patch.operations)
                                    .map(|()| {
                                        let _ = repo.save_snapshot(&patch.target_resource, &current);
                                    })
                                    .unwrap_or_else(|e| {
                                        tracing::warn!(
                                            "Failed to apply patch to {}: {e}",
                                            patch.target_resource
                                        );
                                    });
                            });
                    });
            }
        });

        Ok(())
    })?;

    // Update channel data
    let mut channel_data = repo.load_channel(channel)?;
    izip!(&response.changesets).for_each(|cs| {
        (!channel_data.changesets.contains(&cs.change_id))
            .then(|| channel_data.append_changeset(cs.change_id.clone()));
    });
    repo.save_channel(&channel_data)?;

    // Update sync state
    response
        .current_head
        .as_ref()
        .map(|head| -> Result<()> {
            let mut ss = repo.load_sync_state()?;
            ss.remote_heads
                .insert(channel.to_string(), head.clone());
            repo.save_sync_state(&ss)
        })
        .transpose()?;

    tracing::info!(
        "Pulled {} new changeset(s) for channel '{}'",
        response.changesets.len(),
        channel
    );

    Ok(())
}
