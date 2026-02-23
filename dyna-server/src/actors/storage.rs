//! S3 Storage Actor.
//!
//! This actor encapsulates all interactions with the S3-compatible object store.
//! It handles storing and retrieving patches, channels, and resource snapshots.
//! It uses the `object_store` crate for S3 interactions.

use bytes::Bytes;
use elfo::prelude::*;
use object_store::{ObjectStore, memory::InMemory, path::Path as ObjPath};
use std::sync::Arc;

use crate::messages::*;

/// Create the S3 Storage actor blueprint.
///
/// The `store_url` parameter can be:
/// - `"memory://"` for an in-memory store (testing)
/// - `"s3://bucket-name"` for a real S3 bucket (requires AWS credentials in env)
pub fn new(store: Arc<dyn ObjectStore>) -> Blueprint {
    let store_clone = store.clone();
    ActorGroup::new().exec(move |mut ctx| {
        let store = store_clone.clone();
        async move {
            tracing::info!("S3 Storage actor started");

            while let Some(envelope) = ctx.recv().await {
                msg!(match envelope {
                    // ----------------------------------------------------------
                    // Patch operations
                    // ----------------------------------------------------------
                    (StorePatch { patch }, token) => {
                        let hex = dyna_common::hash::strip_prefix(&patch.hash);
                        let path = ObjPath::from(format!("patches/{}.json", hex));
                        let result = match serde_json::to_vec_pretty(&patch) {
                            Ok(data) => {
                                match store.put(&path, Bytes::from(data).into()).await {
                                    Ok(_) => {
                                        tracing::debug!(hash = %patch.hash, "Stored patch");
                                        StorePatchResult::Ok
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "Failed to store patch");
                                        StorePatchResult::Error(e.to_string())
                                    }
                                }
                            }
                            Err(e) => StorePatchResult::Error(e.to_string()),
                        };
                        ctx.respond(token, result);
                    }

                    (LoadPatch { hash }, token) => {
                        let hex = dyna_common::hash::strip_prefix(&hash);
                        let path = ObjPath::from(format!("patches/{}.json", hex));
                        let result = match store.get(&path).await {
                            Ok(get_result) => {
                                match get_result.bytes().await {
                                    Ok(data) => {
                                        match serde_json::from_slice(&data) {
                                            Ok(patch) => LoadPatchResult::Ok(patch),
                                            Err(e) => LoadPatchResult::Error(
                                                format!("Deserialization error: {}", e),
                                            ),
                                        }
                                    }
                                    Err(e) => LoadPatchResult::Error(e.to_string()),
                                }
                            }
                            Err(object_store::Error::NotFound { .. }) => LoadPatchResult::NotFound,
                            Err(e) => LoadPatchResult::Error(e.to_string()),
                        };
                        ctx.respond(token, result);
                    }

                    // ----------------------------------------------------------
                    // Channel operations
                    // ----------------------------------------------------------
                    (SaveChannel { channel }, token) => {
                        let path = ObjPath::from(format!("channels/{}.json", channel.name));
                        let result = match serde_json::to_vec_pretty(&channel) {
                            Ok(data) => {
                                match store.put(&path, Bytes::from(data).into()).await {
                                    Ok(_) => {
                                        tracing::debug!(name = %channel.name, "Saved channel");
                                        SaveChannelResult::Ok
                                    }
                                    Err(e) => SaveChannelResult::Error(e.to_string()),
                                }
                            }
                            Err(e) => SaveChannelResult::Error(e.to_string()),
                        };
                        ctx.respond(token, result);
                    }

                    (LoadChannel { name }, token) => {
                        let path = ObjPath::from(format!("channels/{}.json", name));
                        let result = match store.get(&path).await {
                            Ok(get_result) => {
                                match get_result.bytes().await {
                                    Ok(data) => {
                                        match serde_json::from_slice(&data) {
                                            Ok(channel) => LoadChannelResult::Ok(channel),
                                            Err(e) => LoadChannelResult::Error(
                                                format!("Deserialization error: {}", e),
                                            ),
                                        }
                                    }
                                    Err(e) => LoadChannelResult::Error(e.to_string()),
                                }
                            }
                            Err(object_store::Error::NotFound { .. }) => {
                                LoadChannelResult::NotFound
                            }
                            Err(e) => LoadChannelResult::Error(e.to_string()),
                        };
                        ctx.respond(token, result);
                    }

                    (ListAllChannels, token) => {
                        let prefix = ObjPath::from("channels/");
                        let result = match store.list_with_delimiter(Some(&prefix)).await {
                            Ok(list_result) => {
                                let mut channels = Vec::new();
                                for obj in &list_result.objects {
                                    match store.get(&obj.location).await {
                                        Ok(get_result) => {
                                            if let Ok(data) = get_result.bytes().await {
                                                if let Ok(channel) =
                                                    serde_json::from_slice(&data)
                                                {
                                                    channels.push(channel);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                error = %e,
                                                path = %obj.location,
                                                "Failed to load channel"
                                            );
                                        }
                                    }
                                }
                                ListChannelsResult::Ok(channels)
                            }
                            Err(e) => ListChannelsResult::Error(e.to_string()),
                        };
                        ctx.respond(token, result);
                    }

                    // ----------------------------------------------------------
                    // Snapshot operations
                    // ----------------------------------------------------------
                    (SaveSnapshot { resource_id, value }, token) => {
                        let path = ObjPath::from(format!("snapshots/{}.json", resource_id));
                        let result = match serde_json::to_vec_pretty(&value) {
                            Ok(data) => {
                                match store.put(&path, Bytes::from(data).into()).await {
                                    Ok(_) => {
                                        tracing::debug!(
                                            resource_id = %resource_id,
                                            "Saved snapshot"
                                        );
                                        SaveSnapshotResult::Ok
                                    }
                                    Err(e) => SaveSnapshotResult::Error(e.to_string()),
                                }
                            }
                            Err(e) => SaveSnapshotResult::Error(e.to_string()),
                        };
                        ctx.respond(token, result);
                    }

                    (LoadSnapshot { resource_id }, token) => {
                        let path = ObjPath::from(format!("snapshots/{}.json", resource_id));
                        let result = match store.get(&path).await {
                            Ok(get_result) => {
                                match get_result.bytes().await {
                                    Ok(data) => {
                                        match serde_json::from_slice(&data) {
                                            Ok(value) => LoadSnapshotResult::Ok(value),
                                            Err(e) => LoadSnapshotResult::Error(
                                                format!("Deserialization error: {}", e),
                                            ),
                                        }
                                    }
                                    Err(e) => LoadSnapshotResult::Error(e.to_string()),
                                }
                            }
                            Err(object_store::Error::NotFound { .. }) => {
                                LoadSnapshotResult::NotFound
                            }
                            Err(e) => LoadSnapshotResult::Error(e.to_string()),
                        };
                        ctx.respond(token, result);
                    }

                    (LoadAllSnapshots, token) => {
                        let prefix = ObjPath::from("snapshots/");
                        let result = match store.list_with_delimiter(Some(&prefix)).await {
                            Ok(list_result) => {
                                let mut snapshots = std::collections::HashMap::new();
                                for obj in &list_result.objects {
                                    let resource_id = obj
                                        .location
                                        .filename()
                                        .unwrap_or_default()
                                        .trim_end_matches(".json")
                                        .to_string();

                                    match store.get(&obj.location).await {
                                        Ok(get_result) => {
                                            if let Ok(data) = get_result.bytes().await {
                                                if let Ok(value) =
                                                    serde_json::from_slice(&data)
                                                {
                                                    snapshots.insert(resource_id, value);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                error = %e,
                                                path = %obj.location,
                                                "Failed to load snapshot"
                                            );
                                        }
                                    }
                                }
                                LoadAllSnapshotsResult::Ok(snapshots)
                            }
                            Err(e) => LoadAllSnapshotsResult::Error(e.to_string()),
                        };
                        ctx.respond(token, result);
                    }
                });
            }

            tracing::info!("S3 Storage actor stopped");
        }
    })
}

/// Create an in-memory object store (for testing and development).
pub fn create_memory_store() -> Arc<dyn ObjectStore> {
    Arc::new(InMemory::new())
}

/// Create an S3 object store from environment variables.
///
/// Required environment variables:
/// - `AWS_ACCESS_KEY_ID`
/// - `AWS_SECRET_ACCESS_KEY`
/// - `AWS_REGION` (or `AWS_DEFAULT_REGION`)
/// - `DYNA_S3_BUCKET` (the bucket name)
/// - `DYNA_S3_ENDPOINT` (optional, for S3-compatible services like MinIO)
pub fn create_s3_store() -> Result<Arc<dyn ObjectStore>, anyhow::Error> {
    let bucket = std::env::var("DYNA_S3_BUCKET")
        .unwrap_or_else(|_| "dyna-store".to_string());

    let mut builder = object_store::aws::AmazonS3Builder::from_env()
        .with_bucket_name(&bucket);

    if let Ok(endpoint) = std::env::var("DYNA_S3_ENDPOINT") {
        builder = builder.with_endpoint(&endpoint).with_allow_http(true);
    }

    let store = builder.build()?;
    Ok(Arc::new(store))
}
