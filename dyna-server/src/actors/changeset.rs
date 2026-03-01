//! Changeset Actor — core business logic for the Dyna server.
//!
//! This elfo actor is the central coordinator for all changeset operations:
//! - **Push**: Validates incoming changesets, appends them to the target channel,
//!   and delegates storage to the S3 Storage Actor.
//! - **Pull**: Retrieves changesets from a channel since a given `since_change_id`.
//! - **Clone**: Returns all channels and their complete changeset history.
//! - **Promote**: Moves changesets from a source channel to `main`, marking
//!   them as immutable using [`promote_changesets`].
//! - **Get Changeset**: Retrieves a single changeset by its `change_id`.
//!
//! The actor maintains an in-memory cache of channels and changesets, backed
//! by the S3 Storage Actor for persistence.

use dyna_core::channel::promote_changesets;
use dyna_core::diff;
use dyna_core::models::{Changeset, Channel};
use dyna_core::protocol::*;
use elfo::prelude::*;
use itertools::{izip, Itertools};

use crate::messages::*;

/// Create the Changeset actor blueprint.
pub fn new() -> Blueprint {
    ActorGroup::new().exec(changeset_actor)
}

async fn changeset_actor(mut ctx: Context) {
    tracing::info!("Changeset actor started");

    ensure_main_channel(&ctx).await;

    while let Some(envelope) = ctx.recv().await {
        msg!(match envelope {
            (HandlePush { channel, changesets, expected_head }, token) => {
                let response = handle_push(&ctx, channel, changesets, expected_head).await;
                ctx.respond(token, response);
            }
            (HandlePull { channel, since_change_id }, token) => {
                let response = handle_pull(&ctx, channel, since_change_id).await;
                ctx.respond(token, response);
            }
            (HandleClone { channel }, token) => {
                let response = handle_clone(&ctx, channel).await;
                ctx.respond(token, response);
            }
            (HandlePromote { source_channel, target_channel }, token) => {
                let response = handle_promote(&ctx, source_channel, target_channel).await;
                ctx.respond(token, response);
            }
            (HandleCreateChannel { name, fork_from }, token) => {
                let response = handle_create_channel(&ctx, name, fork_from).await;
                ctx.respond(token, response);
            }
            (HandleListChannels, token) => {
                let response = handle_list_channels(&ctx).await;
                ctx.respond(token, response);
            }
            (HandleGetChangeset { change_id }, token) => {
                let response = handle_get_changeset(&ctx, change_id).await;
                ctx.respond(token, response);
            }
            (HandleResourceHistory { resource_id }, token) => {
                let response = handle_resource_history(&ctx, resource_id).await;
                ctx.respond(token, response);
            }
        });
    }

    tracing::info!("Changeset actor stopped");
}

/// Helper: load a channel from storage, returning Ok(channel) or Err(error_msg).
async fn load_channel_or_err(ctx: &Context, name: &str) -> Result<Channel, String> {
    ctx.request(LoadChannel { name: name.into() })
        .resolve()
        .await
        .map_err(|e| format!("Request failed: {}", e))
        .and_then(|result| match result {
            LoadChannelResult::Ok(ch) => Ok(ch),
            LoadChannelResult::NotFound => Err(format!("Channel '{}' not found", name)),
            LoadChannelResult::Error(e) => Err(e),
        })
}

/// Helper: load a channel, creating a new one if not found.
async fn load_or_create_channel(ctx: &Context, name: &str) -> Result<Channel, String> {
    ctx.request(LoadChannel { name: name.into() })
        .resolve()
        .await
        .map_err(|e| format!("Request failed: {}", e))
        .map(|result| match result {
            LoadChannelResult::Ok(ch) => ch,
            _ => Channel::new(name),
        })
}

/// Helper: save a channel to storage.
async fn save_channel(ctx: &Context, channel: &Channel) -> Result<(), String> {
    ctx.request(SaveChannel {
        channel: channel.clone(),
    })
    .resolve()
    .await
    .map_err(|e| format!("Request failed: {}", e))
    .and_then(|result| match result {
        SaveChannelResult::Ok => Ok(()),
        SaveChannelResult::Error(e) => Err(e),
    })
}

/// Helper: store a changeset to S3.
async fn store_changeset(ctx: &Context, cs: &Changeset) -> Result<(), String> {
    ctx.request(StoreChangeset {
        changeset: cs.clone(),
    })
    .resolve()
    .await
    .map_err(|e| format!("Request failed: {}", e))
    .and_then(|result| match result {
        StoreChangesetResult::Ok => Ok(()),
        StoreChangesetResult::Error(e) => Err(e),
    })
}

/// Helper: load a changeset from S3.
async fn load_changeset(ctx: &Context, change_id: &str) -> Option<Changeset> {
    ctx.request(LoadChangeset {
        change_id: change_id.into(),
    })
    .resolve()
    .await
    .ok()
    .and_then(|result| match result {
        LoadChangesetResult::Ok(cs) => Some(cs),
        _ => None,
    })
}

/// Ensure the "main" channel exists in storage.
async fn ensure_main_channel(ctx: &Context) {
    ctx.request(LoadChannel {
        name: "main".into(),
    })
    .resolve()
    .await
    .ok()
    .map(|result| match result {
        LoadChannelResult::NotFound => {
            tracing::info!("Creating default 'main' channel");
            Some(Channel::new("main"))
        }
        LoadChannelResult::Ok(_) => {
            tracing::debug!("'main' channel already exists");
            None
        }
        LoadChannelResult::Error(e) => {
            tracing::error!(error = %e, "Failed to check for 'main' channel");
            Some(Channel::new("main"))
        }
    })
    .flatten()
    .map(|ch| async move {
        let _ = ctx
            .request(SaveChannel { channel: ch })
            .resolve()
            .await;
    });
}

/// Handle a push request (changeset-based).
async fn handle_push(
    ctx: &Context,
    channel_name: String,
    changesets: Vec<Changeset>,
    expected_head: Option<String>,
) -> PushResponse {
    let make_err = |channel: &Channel, accepted: usize, msg: String| PushResponse {
        success: false,
        new_head: channel.head_change_id.clone(),
        accepted_count: accepted,
        error: Some(msg),
    };

    // Load or create channel
    let mut channel = match load_or_create_channel(ctx, &channel_name).await {
        Ok(ch) => ch,
        Err(e) => {
            return PushResponse {
                success: false,
                new_head: None,
                accepted_count: 0,
                error: Some(e),
            };
        }
    };

    // Optimistic concurrency check via and_then
    let concurrency_ok = expected_head
        .as_ref()
        .map(|expected| {
            (channel.head_change_id.as_deref() == Some(expected.as_str()))
                .then_some(())
                .ok_or_else(|| {
                    format!(
                        "Concurrent modification: expected head '{}', but found '{}'",
                        &expected[..std::cmp::min(expected.len(), 8)],
                        channel
                            .head_change_id
                            .as_deref()
                            .map(|h| &h[..std::cmp::min(h.len(), 8)])
                            .unwrap_or("(none)")
                    )
                })
        })
        .transpose();

    if let Err(msg) = concurrency_ok {
        return make_err(&channel, 0, msg);
    }

    // Validate and store each changeset, accumulating accepted count
    // We need sequential processing here due to async storage calls
    let mut accepted = 0usize;
    for cs in &changesets {
        // Verify integrity
        if !cs.verify() {
            return make_err(
                &channel,
                accepted,
                format!("Changeset {} failed integrity check", cs.short_change_id()),
            );
        }

        // Store changeset
        if let Err(e) = store_changeset(ctx, cs).await {
            return make_err(
                &channel,
                accepted,
                format!("Failed to store changeset {}: {}", cs.short_change_id(), e),
            );
        }

        // Apply patches to snapshots
        for patch in &cs.patches {
            let save_value = patch
                .result_snapshot
                .clone()
                .map(Ok)
                .unwrap_or_else(|| {
                    // Load current snapshot and apply operations
                    futures::executor::block_on(
                        ctx.request(LoadSnapshot {
                            resource_id: patch.target_resource.clone(),
                        })
                        .resolve(),
                    )
                    .ok()
                    .and_then(|r| match r {
                        LoadSnapshotResult::Ok(v) => Some(v),
                        _ => None,
                    })
                    .unwrap_or_else(|| serde_json::json!({}))
                    .pipe(|mut current| {
                        diff::apply_patch(&mut current, &patch.operations)
                            .map(|()| current)
                            .map_err(|e| e.to_string())
                    })
                });

            if let Ok(value) = save_value {
                let _ = ctx
                    .request(SaveSnapshot {
                        resource_id: patch.target_resource.clone(),
                        value,
                    })
                    .resolve()
                    .await;
            }
        }

        channel.append_changeset(cs.change_id.clone());
        accepted += 1;
    }

    // Save updated channel
    save_channel(ctx, &channel)
        .await
        .map(|()| PushResponse {
            success: true,
            new_head: channel.head_change_id.clone(),
            accepted_count: accepted,
            error: None,
        })
        .unwrap_or_else(|e| make_err(&channel, accepted, format!("Failed to save channel: {}", e)))
}

/// Pipe trait for inline transformations.
trait Pipe: Sized {
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
    {
        f(self)
    }
}

impl<T> Pipe for T {}

/// Handle a pull request (changeset-based).
async fn handle_pull(
    ctx: &Context,
    channel_name: String,
    since_change_id: Option<String>,
) -> PullResponse {
    let empty_response = || PullResponse {
        changesets: vec![],
        current_head: None,
        channel: Channel::new(&channel_name),
    };

    let channel = match load_channel_or_err(ctx, &channel_name).await {
        Ok(ch) => ch,
        Err(_) => return empty_response(),
    };

    // Determine changeset IDs to send using skip_while
    let changeset_ids = since_change_id
        .as_ref()
        .map(|since| {
            izip!(&channel.changesets)
                .skip_while(|id| *id != since)
                .skip(1)
                .cloned()
                .collect_vec()
        })
        .unwrap_or_else(|| channel.changesets.clone());

    // Load changeset objects, filtering out failures
    let mut changesets = Vec::new();
    for id in &changeset_ids {
        load_changeset(ctx, id)
            .await
            .map(|cs| changesets.push(cs))
            .unwrap_or_else(|| {
                tracing::warn!(change_id = %id, "Failed to load changeset during pull");
            });
    }

    PullResponse {
        changesets,
        current_head: channel.head_change_id.clone(),
        channel,
    }
}

/// Handle a clone request (changeset-based).
async fn handle_clone(ctx: &Context, _channel: Option<String>) -> CloneResponse {
    // Load all channels
    let channels = ctx
        .request(ListAllChannels)
        .resolve()
        .await
        .ok()
        .and_then(|r| match r {
            ListChannelsResult::Ok(chs) => Some(chs),
            _ => None,
        })
        .unwrap_or_default();

    // Collect all unique changeset IDs from all channels
    let all_cs_ids = izip!(&channels)
        .flat_map(|ch| izip!(&ch.changesets))
        .cloned()
        .collect::<std::collections::HashSet<_>>();

    // Load all changesets, sort by creation time
    let mut changesets = Vec::new();
    for id in &all_cs_ids {
        load_changeset(ctx, id)
            .await
            .map(|cs| changesets.push(cs));
    }
    changesets.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    // Load all snapshots
    let snapshots = ctx
        .request(LoadAllSnapshots)
        .resolve()
        .await
        .ok()
        .and_then(|r| match r {
            LoadAllSnapshotsResult::Ok(s) => Some(s),
            _ => None,
        })
        .unwrap_or_default();

    CloneResponse {
        channels,
        changesets,
        snapshots,
    }
}

/// Handle a promote request (changeset-based).
async fn handle_promote(
    ctx: &Context,
    source_name: String,
    target_name: String,
) -> PromoteResponse {
    let make_err = |msg: String, head: Option<String>| PromoteResponse {
        success: false,
        promoted_changesets: vec![],
        new_head: head,
        error: Some(msg),
    };

    let source = match load_channel_or_err(ctx, &source_name).await {
        Ok(ch) => ch,
        Err(e) => return make_err(e, None),
    };

    let mut target = match load_channel_or_err(ctx, &target_name).await {
        Ok(ch) => ch,
        Err(e) => return make_err(e, None),
    };

    let promoted_ids = match promote_changesets(&source, &mut target) {
        Ok(ids) => ids,
        Err(e) => return make_err(e.to_string(), None),
    };

    // Mark promoted changesets as immutable
    for id in &promoted_ids {
        if let Some(mut cs) = load_changeset(ctx, id).await {
            cs.immutable = true;
            let _ = store_changeset(ctx, &cs).await;
        }
    }

    // Save updated target channel
    save_channel(ctx, &target)
        .await
        .map(|()| PromoteResponse {
            success: true,
            promoted_changesets: promoted_ids,
            new_head: target.head_change_id.clone(),
            error: None,
        })
        .unwrap_or_else(|e| make_err(e, target.head_change_id.clone()))
}

/// Handle a create channel request.
async fn handle_create_channel(
    ctx: &Context,
    name: String,
    fork_from: Option<String>,
) -> CreateChannelResponse {
    let make_err = |msg: String| CreateChannelResponse {
        success: false,
        channel: Channel::new(&name),
        error: Some(msg),
    };

    // Check if channel already exists
    let exists = ctx
        .request(LoadChannel { name: name.clone() })
        .resolve()
        .await
        .ok()
        .and_then(|r| match r {
            LoadChannelResult::Ok(_) => Some(true),
            _ => None,
        })
        .unwrap_or(false);

    if exists {
        return make_err(format!("Channel '{}' already exists", name));
    }

    // Create channel, optionally forking from source
    let channel = match fork_from {
        Some(source_name) => {
            load_channel_or_err(ctx, &source_name)
                .await
                .map(|source| {
                    let mut new_channel = Channel::new(&name);
                    new_channel.changesets = source.changesets.clone();
                    new_channel.head_change_id = source.head_change_id.clone();
                    new_channel
                })
                .unwrap_or_else(|e| {
                    // Return early via a sentinel; we'll check below
                    Channel::new(&format!("__error__{}", e))
                })
        }
        None => Channel::new(&name),
    };

    // Check for error sentinel
    if channel.name.starts_with("__error__") {
        let err_msg = channel.name.strip_prefix("__error__").unwrap_or("Unknown");
        return make_err(err_msg.to_string());
    }

    save_channel(ctx, &channel)
        .await
        .map(|()| CreateChannelResponse {
            success: true,
            channel,
            error: None,
        })
        .unwrap_or_else(|e| make_err(format!("Failed to save channel: {}", e)))
}

/// Handle a list channels request.
async fn handle_list_channels(ctx: &Context) -> ListChannelsResponse {
    ctx.request(ListAllChannels)
        .resolve()
        .await
        .ok()
        .and_then(|r| match r {
            ListChannelsResult::Ok(channels) => Some(ListChannelsResponse { channels }),
            _ => None,
        })
        .unwrap_or_else(|| ListChannelsResponse {
            channels: vec![],
        })
}

/// Handle a resource history query.
///
/// Walks all channels and their changesets to find every changeset that
/// contains a patch targeting the given resource_id. Returns entries in
/// reverse chronological order.
async fn handle_resource_history(
    ctx: &Context,
    resource_id: String,
) -> ResourceHistoryResponse {
    let channels = ctx
        .request(ListAllChannels)
        .resolve()
        .await
        .ok()
        .and_then(|r| match r {
            ListChannelsResult::Ok(chs) => Some(chs),
            _ => None,
        })
        .unwrap_or_default();

    let mut entries = Vec::new();

    for channel in &channels {
        for cs_id in &channel.changesets {
            if let Some(cs) = load_changeset(ctx, cs_id).await {
                // Check if any patch targets this resource
                let matching_patches: Vec<_> = cs
                    .patches
                    .iter()
                    .filter(|p| p.target_resource == resource_id)
                    .collect();

                if !matching_patches.is_empty() {
                    let operations: Vec<serde_json::Value> = matching_patches
                        .iter()
                        .flat_map(|p| {
                            p.operations.iter().map(|op| {
                                serde_json::to_value(op).unwrap_or(serde_json::json!(null))
                            })
                        })
                        .collect();

                    entries.push(ResourceHistoryEntry {
                        change_id: cs.change_id.clone(),
                        commit_hash: cs.commit_hash.clone(),
                        message: cs.message.clone(),
                        author: cs.author.clone(),
                        timestamp: cs.created_at.to_rfc3339(),
                        channel: channel.name.clone(),
                        operations,
                    });
                }
            }
        }
    }

    // Sort by timestamp descending (most recent first)
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    ResourceHistoryResponse {
        resource_id,
        entries,
        error: None,
    }
}

/// Handle a get changeset detail request.
async fn handle_get_changeset(ctx: &Context, change_id: String) -> GetChangesetResponse {
    ctx.request(LoadChangeset {
        change_id: change_id.clone(),
    })
    .resolve()
    .await
    .ok()
    .map(|result| match result {
        LoadChangesetResult::Ok(cs) => GetChangesetResponse {
            changeset: Some(cs),
            error: None,
        },
        LoadChangesetResult::NotFound => GetChangesetResponse {
            changeset: None,
            error: Some(format!("Changeset '{}' not found", change_id)),
        },
        LoadChangesetResult::Error(e) => GetChangesetResponse {
            changeset: None,
            error: Some(e),
        },
    })
    .unwrap_or_else(|| GetChangesetResponse {
        changeset: None,
        error: Some("Failed to load changeset".into()),
    })
}
