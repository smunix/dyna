//! Changeset Manager Actor.
//!
//! This actor is the core business logic actor. It manages channels, validates
//! patches, handles push/pull operations, and orchestrates conflict resolution.
//! It delegates all S3 I/O to the Storage actor.

use dyna_common::channel::promote_patches;
use dyna_common::diff;
use dyna_common::models::{Channel, Patch};
use dyna_common::protocol::*;
use elfo::prelude::*;

use crate::messages::*;

/// Create the Changeset Manager actor blueprint.
pub fn new() -> Blueprint {
    ActorGroup::new().exec(changeset_manager)
}

async fn changeset_manager(mut ctx: Context) {
    tracing::info!("Changeset Manager actor started");

    // Ensure the default "main" channel exists on startup.
    ensure_main_channel(&ctx).await;

    while let Some(envelope) = ctx.recv().await {
        msg!(match envelope {
            // ----------------------------------------------------------
            // Push: accept patches from a client
            // ----------------------------------------------------------
            (HandlePush { channel, patches, expected_head }, token) => {
                let response = handle_push(&ctx, channel, patches, expected_head).await;
                ctx.respond(token, response);
            }

            // ----------------------------------------------------------
            // Pull: send patches to a client
            // ----------------------------------------------------------
            (HandlePull { channel, since_hash }, token) => {
                let response = handle_pull(&ctx, channel, since_hash).await;
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
            // Promote: merge patches from one channel to another
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
        });
    }

    tracing::info!("Changeset Manager actor stopped");
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

/// Handle a push request.
async fn handle_push(
    ctx: &Context,
    channel_name: String,
    patches: Vec<Patch>,
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
        Ok(LoadChannelResult::NotFound) => {
            // Auto-create the channel
            Channel::new(&channel_name)
        }
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
        if channel.head.as_deref() != Some(expected.as_str()) {
            return PushResponse {
                success: false,
                new_head: channel.head.clone(),
                accepted_count: 0,
                error: Some(format!(
                    "Concurrent modification: expected head '{}', but found '{}'",
                    &expected[..std::cmp::min(expected.len(), 19)],
                    channel
                        .head
                        .as_deref()
                        .map(|h| &h[..std::cmp::min(h.len(), 19)])
                        .unwrap_or("(none)")
                )),
            };
        }
    }

    // Validate and store each patch
    let mut accepted = 0;
    for patch in &patches {
        // Verify patch integrity
        if !patch.verify() {
            return PushResponse {
                success: false,
                new_head: channel.head.clone(),
                accepted_count: accepted,
                error: Some(format!(
                    "Patch {} failed integrity check",
                    &patch.hash[..std::cmp::min(patch.hash.len(), 19)]
                )),
            };
        }

        // Store the patch
        let store_result = ctx
            .request(StorePatch {
                patch: patch.clone(),
            })
            .resolve()
            .await;

        match store_result {
            Ok(StorePatchResult::Ok) => {}
            _ => {
                return PushResponse {
                    success: false,
                    new_head: channel.head.clone(),
                    accepted_count: accepted,
                    error: Some(format!(
                        "Failed to store patch {}",
                        &patch.hash[..std::cmp::min(patch.hash.len(), 19)]
                    )),
                };
            }
        }

        // Apply the patch to the resource snapshot
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

        // Append to channel
        channel.append_patch(patch.hash.clone());
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
            new_head: channel.head.clone(),
            accepted_count: accepted,
            error: None,
        },
        _ => PushResponse {
            success: false,
            new_head: channel.head.clone(),
            accepted_count: accepted,
            error: Some("Failed to save channel metadata".into()),
        },
    }
}

/// Handle a pull request.
async fn handle_pull(
    ctx: &Context,
    channel_name: String,
    since_hash: Option<String>,
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
                patches: vec![],
                current_head: None,
                channel: Channel::new(&channel_name),
            };
        }
    };

    // Determine which patches to send
    let patch_hashes = channel.patches_since(since_hash.as_deref());

    // Load the patch objects
    let mut patches = Vec::new();
    for hash in &patch_hashes {
        let load_result = ctx
            .request(LoadPatch {
                hash: hash.clone(),
            })
            .resolve()
            .await;

        if let Ok(LoadPatchResult::Ok(patch)) = load_result {
            patches.push(patch);
        } else {
            tracing::warn!(hash = %hash, "Failed to load patch during pull");
        }
    }

    PullResponse {
        patches,
        current_head: channel.head.clone(),
        channel,
    }
}

/// Handle a clone request.
async fn handle_clone(ctx: &Context, _channel: Option<String>) -> CloneResponse {
    // Load all channels
    let channels_result = ctx.request(ListAllChannels).resolve().await;
    let channels = match channels_result {
        Ok(ListChannelsResult::Ok(chs)) => chs,
        _ => vec![],
    };

    // Load all patches from all channels
    let mut all_patch_hashes = std::collections::HashSet::new();
    for ch in &channels {
        for hash in &ch.patches {
            all_patch_hashes.insert(hash.clone());
        }
    }

    let mut patches = Vec::new();
    for hash in &all_patch_hashes {
        let load_result = ctx
            .request(LoadPatch {
                hash: hash.clone(),
            })
            .resolve()
            .await;

        if let Ok(LoadPatchResult::Ok(patch)) = load_result {
            patches.push(patch);
        }
    }

    // Sort patches by timestamp for consistent ordering
    patches.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    // Load all snapshots
    let snapshots_result = ctx.request(LoadAllSnapshots).resolve().await;
    let snapshots = match snapshots_result {
        Ok(LoadAllSnapshotsResult::Ok(s)) => s,
        _ => std::collections::HashMap::new(),
    };

    CloneResponse {
        channels,
        patches,
        snapshots,
    }
}

/// Handle a promote request.
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
                promoted_patches: vec![],
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
                promoted_patches: vec![],
                new_head: None,
                error: Some(format!("Target channel '{}' not found", target_name)),
            };
        }
    };

    // Promote patches
    match promote_patches(&source, &mut target) {
        Ok(promoted) => {
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
                    promoted_patches: promoted,
                    new_head: target.head.clone(),
                    error: None,
                },
                _ => PromoteResponse {
                    success: false,
                    promoted_patches: vec![],
                    new_head: None,
                    error: Some("Failed to save target channel".into()),
                },
            }
        }
        Err(e) => PromoteResponse {
            success: false,
            promoted_patches: vec![],
            new_head: target.head.clone(),
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
                new_channel.patches = source.patches.clone();
                new_channel.head = source.head.clone();
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
