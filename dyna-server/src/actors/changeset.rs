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

use crate::messages::*;

/// Create the Changeset actor blueprint.
pub fn new() -> Blueprint {
    ActorGroup::new().exec(changeset_actor)
}

async fn changeset_actor(mut ctx: Context) {
    tracing::info!("Changeset actor started");

    // Ensure the default "main" channel exists on startup.
    ensure_main_channel(&ctx).await;

    while let Some(envelope) = ctx.recv().await {
        msg!(match envelope {
            // ----------------------------------------------------------
            // Push: accept changesets from a client
            // ----------------------------------------------------------
            (HandlePush { channel, changesets, expected_head }, token) => {
                let response = handle_push(&ctx, channel, changesets, expected_head).await;
                ctx.respond(token, response);
            }

            // ----------------------------------------------------------
            // Pull: send changesets to a client
            // ----------------------------------------------------------
            (HandlePull { channel, since_change_id }, token) => {
                let response = handle_pull(&ctx, channel, since_change_id).await;
                ctx.respond(token, response);
            }

            // ----------------------------------------------------------
            // Clone: send all data to a client
            // ----------------------------------------------------------
            (HandleClone { channel }, token) => {
                let response = handle_clone(&ctx, channel).await;
                ctx.respond(token, response);
            }

            // ----------------------------------------------------------
            // Promote: merge changesets from one channel to another
            // ----------------------------------------------------------
            (HandlePromote { source_channel, target_channel }, token) => {
                let response = handle_promote(&ctx, source_channel, target_channel).await;
                ctx.respond(token, response);
            }

            // ----------------------------------------------------------
            // Create Channel
            // ----------------------------------------------------------
            (HandleCreateChannel { name, fork_from }, token) => {
                let response = handle_create_channel(&ctx, name, fork_from).await;
                ctx.respond(token, response);
            }

            // ----------------------------------------------------------
            // List Channels
            // ----------------------------------------------------------
            (HandleListChannels, token) => {
                let response = handle_list_channels(&ctx).await;
                ctx.respond(token, response);
            }

            // ----------------------------------------------------------
            // Get Changeset Detail
            // ----------------------------------------------------------
            (HandleGetChangeset { change_id }, token) => {
                let response = handle_get_changeset(&ctx, change_id).await;
                ctx.respond(token, response);
            }
        });
    }

    tracing::info!("Changeset actor stopped");
}

/// Ensure the "main" channel exists in storage.
async fn ensure_main_channel(ctx: &Context) {
    let result = ctx
        .request(LoadChannel {
            name: "main".into(),
        })
        .resolve()
        .await;

    match result {
        Ok(LoadChannelResult::NotFound) | Err(_) => {
            let main_channel = Channel::new("main");
            let _ = ctx
                .request(SaveChannel {
                    channel: main_channel,
                })
                .resolve()
                .await;
            tracing::info!("Created default 'main' channel");
        }
        Ok(LoadChannelResult::Ok(_)) => {
            tracing::debug!("'main' channel already exists");
        }
        Ok(LoadChannelResult::Error(e)) => {
            tracing::error!(error = %e, "Failed to check for 'main' channel");
        }
    }
}

/// Handle a push request (changeset-based).
async fn handle_push(
    ctx: &Context,
    channel_name: String,
    changesets: Vec<Changeset>,
    expected_head: Option<String>,
) -> PushResponse {
    // Load the channel
    let channel_result = ctx
        .request(LoadChannel {
            name: channel_name.clone(),
        })
        .resolve()
        .await;

    let mut channel = match channel_result {
        Ok(LoadChannelResult::Ok(ch)) => ch,
        Ok(LoadChannelResult::NotFound) => Channel::new(&channel_name),
        _ => {
            return PushResponse {
                success: false,
                new_head: None,
                accepted_count: 0,
                error: Some("Failed to load channel".into()),
            };
        }
    };

    // Optimistic concurrency check
    if let Some(expected) = &expected_head {
        if channel.head_change_id.as_deref() != Some(expected.as_str()) {
            return PushResponse {
                success: false,
                new_head: channel.head_change_id.clone(),
                accepted_count: 0,
                error: Some(format!(
                    "Concurrent modification: expected head '{}', but found '{}'",
                    &expected[..std::cmp::min(expected.len(), 8)],
                    channel
                        .head_change_id
                        .as_deref()
                        .map(|h| &h[..std::cmp::min(h.len(), 8)])
                        .unwrap_or("(none)")
                )),
            };
        }
    }

    // Validate and store each changeset
    let mut accepted = 0;
    for cs in &changesets {
        // Verify changeset integrity
        if !cs.verify() {
            return PushResponse {
                success: false,
                new_head: channel.head_change_id.clone(),
                accepted_count: accepted,
                error: Some(format!(
                    "Changeset {} failed integrity check",
                    cs.short_change_id()
                )),
            };
        }

        // Store the changeset in S3
        let store_result = ctx
            .request(StoreChangeset {
                changeset: cs.clone(),
            })
            .resolve()
            .await;

        match store_result {
            Ok(StoreChangesetResult::Ok) => {}
            _ => {
                return PushResponse {
                    success: false,
                    new_head: channel.head_change_id.clone(),
                    accepted_count: accepted,
                    error: Some(format!(
                        "Failed to store changeset {}",
                        cs.short_change_id()
                    )),
                };
            }
        }

        // Apply each patch in the changeset to resource snapshots
        for patch in &cs.patches {
            if let Some(result_snapshot) = &patch.result_snapshot {
                let _ = ctx
                    .request(SaveSnapshot {
                        resource_id: patch.target_resource.clone(),
                        value: result_snapshot.clone(),
                    })
                    .resolve()
                    .await;
            } else {
                // Compute the result by applying operations
                let snapshot_result = ctx
                    .request(LoadSnapshot {
                        resource_id: patch.target_resource.clone(),
                    })
                    .resolve()
                    .await;

                let mut current = match snapshot_result {
                    Ok(LoadSnapshotResult::Ok(v)) => v,
                    _ => serde_json::json!({}),
                };

                if diff::apply_patch(&mut current, &patch.operations).is_ok() {
                    let _ = ctx
                        .request(SaveSnapshot {
                            resource_id: patch.target_resource.clone(),
                            value: current,
                        })
                        .resolve()
                        .await;
                }
            }
        }

        // Append changeset to channel
        channel.append_changeset(cs.change_id.clone());
        accepted += 1;
    }

    // Save the updated channel
    let save_result = ctx
        .request(SaveChannel {
            channel: channel.clone(),
        })
        .resolve()
        .await;

    match save_result {
        Ok(SaveChannelResult::Ok) => PushResponse {
            success: true,
            new_head: channel.head_change_id.clone(),
            accepted_count: accepted,
            error: None,
        },
        _ => PushResponse {
            success: false,
            new_head: channel.head_change_id.clone(),
            accepted_count: accepted,
            error: Some("Failed to save channel metadata".into()),
        },
    }
}

/// Handle a pull request (changeset-based).
async fn handle_pull(
    ctx: &Context,
    channel_name: String,
    since_change_id: Option<String>,
) -> PullResponse {
    // Load the channel
    let channel_result = ctx
        .request(LoadChannel {
            name: channel_name.clone(),
        })
        .resolve()
        .await;

    let channel = match channel_result {
        Ok(LoadChannelResult::Ok(ch)) => ch,
        _ => {
            return PullResponse {
                changesets: vec![],
                current_head: None,
                channel: Channel::new(&channel_name),
            };
        }
    };

    // Determine which changesets to send (those after since_change_id)
    let changeset_ids: Vec<String> = if let Some(ref since) = since_change_id {
        let mut found = false;
        channel
            .changesets
            .iter()
            .filter(|id| {
                if found {
                    return true;
                }
                if *id == since {
                    found = true;
                }
                false
            })
            .cloned()
            .collect()
    } else {
        channel.changesets.clone()
    };

    // Load the changeset objects
    let mut changesets = Vec::new();
    for id in &changeset_ids {
        let load_result = ctx
            .request(LoadChangeset {
                change_id: id.clone(),
            })
            .resolve()
            .await;

        if let Ok(LoadChangesetResult::Ok(cs)) = load_result {
            changesets.push(cs);
        } else {
            tracing::warn!(change_id = %id, "Failed to load changeset during pull");
        }
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
    let channels_result = ctx.request(ListAllChannels).resolve().await;
    let channels = match channels_result {
        Ok(ListChannelsResult::Ok(chs)) => chs,
        _ => vec![],
    };

    // Collect all unique changeset IDs from all channels
    let mut all_cs_ids = std::collections::HashSet::new();
    for ch in &channels {
        for id in &ch.changesets {
            all_cs_ids.insert(id.clone());
        }
    }

    // Load all changesets
    let mut changesets = Vec::new();
    for id in &all_cs_ids {
        let load_result = ctx
            .request(LoadChangeset {
                change_id: id.clone(),
            })
            .resolve()
            .await;

        if let Ok(LoadChangesetResult::Ok(cs)) = load_result {
            changesets.push(cs);
        }
    }

    // Sort changesets by creation time for consistent ordering
    changesets.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    // Load all snapshots
    let snapshots_result = ctx.request(LoadAllSnapshots).resolve().await;
    let snapshots = match snapshots_result {
        Ok(LoadAllSnapshotsResult::Ok(s)) => s,
        _ => std::collections::HashMap::new(),
    };

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
    // Load source channel
    let source_result = ctx
        .request(LoadChannel {
            name: source_name.clone(),
        })
        .resolve()
        .await;

    let source = match source_result {
        Ok(LoadChannelResult::Ok(ch)) => ch,
        _ => {
            return PromoteResponse {
                success: false,
                promoted_changesets: vec![],
                new_head: None,
                error: Some(format!("Source channel '{}' not found", source_name)),
            };
        }
    };

    // Load target channel
    let target_result = ctx
        .request(LoadChannel {
            name: target_name.clone(),
        })
        .resolve()
        .await;

    let mut target = match target_result {
        Ok(LoadChannelResult::Ok(ch)) => ch,
        _ => {
            return PromoteResponse {
                success: false,
                promoted_changesets: vec![],
                new_head: None,
                error: Some(format!("Target channel '{}' not found", target_name)),
            };
        }
    };

    // Promote changesets
    match promote_changesets(&source, &mut target) {
        Ok(promoted_ids) => {
            // Mark promoted changesets as immutable in storage
            for id in &promoted_ids {
                let load_result = ctx
                    .request(LoadChangeset {
                        change_id: id.clone(),
                    })
                    .resolve()
                    .await;

                if let Ok(LoadChangesetResult::Ok(mut cs)) = load_result {
                    cs.immutable = true;
                    let _ = ctx
                        .request(StoreChangeset { changeset: cs })
                        .resolve()
                        .await;
                }
            }

            // Save the updated target channel
            let save_result = ctx
                .request(SaveChannel {
                    channel: target.clone(),
                })
                .resolve()
                .await;

            match save_result {
                Ok(SaveChannelResult::Ok) => PromoteResponse {
                    success: true,
                    promoted_changesets: promoted_ids,
                    new_head: target.head_change_id.clone(),
                    error: None,
                },
                _ => PromoteResponse {
                    success: false,
                    promoted_changesets: vec![],
                    new_head: None,
                    error: Some("Failed to save target channel".into()),
                },
            }
        }
        Err(e) => PromoteResponse {
            success: false,
            promoted_changesets: vec![],
            new_head: target.head_change_id.clone(),
            error: Some(e.to_string()),
        },
    }
}

/// Handle a create channel request.
async fn handle_create_channel(
    ctx: &Context,
    name: String,
    fork_from: Option<String>,
) -> CreateChannelResponse {
    // Check if channel already exists
    let existing = ctx
        .request(LoadChannel {
            name: name.clone(),
        })
        .resolve()
        .await;

    if let Ok(LoadChannelResult::Ok(_)) = existing {
        return CreateChannelResponse {
            success: false,
            channel: Channel::new(&name),
            error: Some(format!("Channel '{}' already exists", name)),
        };
    }

    // Create the channel
    let channel = if let Some(source_name) = fork_from {
        let source_result = ctx
            .request(LoadChannel {
                name: source_name.clone(),
            })
            .resolve()
            .await;

        match source_result {
            Ok(LoadChannelResult::Ok(source)) => {
                let mut new_channel = Channel::new(&name);
                new_channel.changesets = source.changesets.clone();
                new_channel.head_change_id = source.head_change_id.clone();
                new_channel
            }
            _ => {
                return CreateChannelResponse {
                    success: false,
                    channel: Channel::new(&name),
                    error: Some(format!("Source channel '{}' not found", source_name)),
                };
            }
        }
    } else {
        Channel::new(&name)
    };

    // Save the new channel
    let save_result = ctx
        .request(SaveChannel {
            channel: channel.clone(),
        })
        .resolve()
        .await;

    match save_result {
        Ok(SaveChannelResult::Ok) => CreateChannelResponse {
            success: true,
            channel,
            error: None,
        },
        _ => CreateChannelResponse {
            success: false,
            channel: Channel::new(&name),
            error: Some("Failed to save channel".into()),
        },
    }
}

/// Handle a list channels request.
async fn handle_list_channels(ctx: &Context) -> ListChannelsResponse {
    let result = ctx.request(ListAllChannels).resolve().await;
    match result {
        Ok(ListChannelsResult::Ok(channels)) => ListChannelsResponse { channels },
        _ => ListChannelsResponse {
            channels: vec![],
        },
    }
}

/// Handle a get changeset detail request.
async fn handle_get_changeset(ctx: &Context, change_id: String) -> GetChangesetResponse {
    let load_result = ctx
        .request(LoadChangeset {
            change_id: change_id.clone(),
        })
        .resolve()
        .await;

    match load_result {
        Ok(LoadChangesetResult::Ok(cs)) => GetChangesetResponse {
            changeset: Some(cs),
            error: None,
        },
        Ok(LoadChangesetResult::NotFound) => GetChangesetResponse {
            changeset: None,
            error: Some(format!("Changeset '{}' not found", change_id)),
        },
        _ => GetChangesetResponse {
            changeset: None,
            error: Some("Failed to load changeset".into()),
        },
    }
}
