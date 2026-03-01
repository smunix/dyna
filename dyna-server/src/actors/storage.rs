//! S3 Storage Actor — persistent object storage for the Dyna server.
//!
//! This elfo actor encapsulates all interactions with the S3-compatible object
//! store via the `object_store` crate. It handles storing and retrieving:
//! - **Changesets** under `changesets/<commit_hash>.json.gz`
//! - **Channels** under `channels/<name>.json.gz`
//! - **Snapshots** under `snapshots/<resource_id>.json.gz`
//!
//! ## Compression
//!
//! All data is stored gzip-compressed to reduce storage footprint. Reads use
//! transparent decompression via `dyna_core::compression`, so they are
//! backwards-compatible with existing uncompressed data.
//!
//! In development mode, an in-memory store is used. In production, configure
//! `AmazonS3Builder::from_env()` for real S3 access.

use bytes::Bytes;
use dyna_core::compression;
use itertools::izip;
use elfo::prelude::*;
use object_store::{ObjectStore, memory::InMemory, path::Path as ObjPath};
use std::sync::Arc;

use crate::messages::*;

/// Helper: serialize to JSON, gzip-compress, and put to object store.
async fn store_json<T: serde::Serialize>(
    store: &dyn ObjectStore,
    path: &ObjPath,
    value: &T,
) -> Result<(), String> {
    compression::compress_json_pretty(value)
        .map_err(|e| e.to_string())
        .map(Bytes::from)
        .and_then(|compressed| {
            // Use block_on since object_store::put is async but we need
            // to chain it functionally. We're already inside an async context
            // so we use a direct await approach instead.
            Ok((path.clone(), compressed))
        })
        .map_err(|e: String| e)?;

    let compressed = compression::compress_json_pretty(value).map_err(|e| e.to_string())?;
    store
        .put(path, Bytes::from(compressed).into())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Helper: get from object store, transparently decompress, and deserialize.
async fn load_json<T: serde::de::DeserializeOwned>(
    store: &dyn ObjectStore,
    path: &ObjPath,
) -> Result<Option<T>, String> {
    match store.get(path).await {
        Ok(get_result) => get_result
            .bytes()
            .await
            .map_err(|e| e.to_string())
            .and_then(|data| {
                compression::decompress_json(&data)
                    .map_err(|e| format!("Deserialization/decompression error: {}", e))
            })
            .map(Some),
        Err(object_store::Error::NotFound { .. }) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Helper: list objects under a prefix, load each (with transparent
/// decompression), and deserialize.
async fn list_and_load_all<T: serde::de::DeserializeOwned>(
    store: &dyn ObjectStore,
    prefix: &ObjPath,
) -> Result<Vec<(String, T)>, String> {
    let list_result = store
        .list_with_delimiter(Some(prefix))
        .await
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for obj in &list_result.objects {
        let name = obj
            .location
            .filename()
            .unwrap_or_default()
            .trim_end_matches(".json.gz")
            .trim_end_matches(".json")
            .to_string();

        let loaded = store
            .get(&obj.location)
            .await
            .ok()
            .and_then(|gr| futures::executor::block_on(gr.bytes()).ok())
            .and_then(|data| compression::decompress_json::<T>(&data).ok())
            .map(|value| (name, value));

        if let Some(pair) = loaded {
            results.push(pair);
        }
    }
    Ok(results)
}

/// Create the S3 Storage actor blueprint.
pub fn new(store: Arc<dyn ObjectStore>) -> Blueprint {
    let store_clone = store.clone();
    ActorGroup::new().exec(move |mut ctx| {
        let store = store_clone.clone();
        async move {
            tracing::info!("S3 Storage actor started (with gzip compression)");

            while let Some(envelope) = ctx.recv().await {
                msg!(match envelope {
                    // ----------------------------------------------------------
                    // Changeset operations
                    // ----------------------------------------------------------
                    (StoreChangeset { changeset }, token) => {
                        let path = ObjPath::from(format!(
                            "changesets/{}.json.gz",
                            changeset.change_id
                        ));
                        let result = store_json(store.as_ref(), &path, &changeset)
                            .await
                            .map(|()| {
                                tracing::debug!(change_id = %changeset.change_id, "Stored changeset (compressed)");
                                StoreChangesetResult::Ok
                            })
                            .unwrap_or_else(|e| {
                                tracing::error!(error = %e, "Failed to store changeset");
                                StoreChangesetResult::Error(e)
                            });
                        ctx.respond(token, result);
                    }

                    (LoadChangeset { change_id }, token) => {
                        // Try compressed path first, fall back to uncompressed
                        let gz_path = ObjPath::from(format!("changesets/{}.json.gz", change_id));
                        let plain_path = ObjPath::from(format!("changesets/{}.json", change_id));
                        let result = match load_json(store.as_ref(), &gz_path).await {
                            Ok(Some(v)) => LoadChangesetResult::Ok(v),
                            Ok(None) => {
                                match load_json(store.as_ref(), &plain_path).await {
                                    Ok(Some(v)) => LoadChangesetResult::Ok(v),
                                    Ok(None) => LoadChangesetResult::NotFound,
                                    Err(e) => LoadChangesetResult::Error(e),
                                }
                            }
                            Err(e) => LoadChangesetResult::Error(e),
                        };
                        ctx.respond(token, result);
                    }

                    // ----------------------------------------------------------
                    // Channel operations
                    // ----------------------------------------------------------
                    (SaveChannel { channel }, token) => {
                        let path = ObjPath::from(format!("channels/{}.json.gz", channel.name));
                        let result = store_json(store.as_ref(), &path, &channel)
                            .await
                            .map(|()| {
                                tracing::debug!(name = %channel.name, "Saved channel (compressed)");
                                SaveChannelResult::Ok
                            })
                            .unwrap_or_else(SaveChannelResult::Error);
                        ctx.respond(token, result);
                    }

                    (LoadChannel { name }, token) => {
                        let gz_path = ObjPath::from(format!("channels/{}.json.gz", name));
                        let plain_path = ObjPath::from(format!("channels/{}.json", name));
                        let result = match load_json(store.as_ref(), &gz_path).await {
                            Ok(Some(v)) => LoadChannelResult::Ok(v),
                            Ok(None) => {
                                match load_json(store.as_ref(), &plain_path).await {
                                    Ok(Some(v)) => LoadChannelResult::Ok(v),
                                    Ok(None) => LoadChannelResult::NotFound,
                                    Err(e) => LoadChannelResult::Error(e),
                                }
                            }
                            Err(e) => LoadChannelResult::Error(e),
                        };
                        ctx.respond(token, result);
                    }

                    (ListAllChannels, token) => {
                        let prefix = ObjPath::from("channels/");
                        let result = list_and_load_all::<dyna_core::models::Channel>(
                            store.as_ref(),
                            &prefix,
                        )
                        .await
                        .map(|pairs| {
                            izip!(pairs).map(|(_name, ch)| ch).collect::<Vec<_>>()
                        })
                        .map(ListChannelsResult::Ok)
                        .unwrap_or_else(ListChannelsResult::Error);

                        ctx.respond(token, result);
                    }

                    // ----------------------------------------------------------
                    // Snapshot operations
                    // ----------------------------------------------------------
                    (SaveSnapshot { channel, resource_id, value }, token) => {
                        let path = ObjPath::from(format!("snapshots/{}/{}.json.gz", channel, resource_id));
                        let result = store_json(store.as_ref(), &path, &value)
                            .await
                            .map(|()| {
                                tracing::debug!(channel = %channel, resource_id = %resource_id, "Saved snapshot (compressed)");
                                SaveSnapshotResult::Ok
                            })
                            .unwrap_or_else(SaveSnapshotResult::Error);
                        ctx.respond(token, result);
                    }

                    (LoadSnapshot { channel, resource_id }, token) => {
                        let gz_path = ObjPath::from(format!("snapshots/{}/{}.json.gz", channel, resource_id));
                        let plain_path = ObjPath::from(format!("snapshots/{}/{}.json", channel, resource_id));
                        // Also try legacy global path for migration
                        let legacy_gz = ObjPath::from(format!("snapshots/{}.json.gz", resource_id));
                        let legacy_plain = ObjPath::from(format!("snapshots/{}.json", resource_id));
                        let result = match load_json(store.as_ref(), &gz_path).await {
                            Ok(Some(v)) => LoadSnapshotResult::Ok(v),
                            Ok(None) => {
                                match load_json(store.as_ref(), &plain_path).await {
                                    Ok(Some(v)) => LoadSnapshotResult::Ok(v),
                                    Ok(None) => {
                                        // Try legacy global paths for backward compat
                                        match load_json(store.as_ref(), &legacy_gz).await {
                                            Ok(Some(v)) => LoadSnapshotResult::Ok(v),
                                            Ok(None) => {
                                                match load_json(store.as_ref(), &legacy_plain).await {
                                                    Ok(Some(v)) => LoadSnapshotResult::Ok(v),
                                                    Ok(None) => LoadSnapshotResult::NotFound,
                                                    Err(e) => LoadSnapshotResult::Error(e),
                                                }
                                            }
                                            Err(e) => LoadSnapshotResult::Error(e),
                                        }
                                    }
                                    Err(e) => LoadSnapshotResult::Error(e),
                                }
                            }
                            Err(e) => LoadSnapshotResult::Error(e),
                        };
                        ctx.respond(token, result);
                    }

                    (LoadAllSnapshots { channel }, token) => {
                        let prefix = ObjPath::from(format!("snapshots/{}/", channel));
                        let result = list_and_load_all::<serde_json::Value>(
                            store.as_ref(),
                            &prefix,
                        )
                        .await
                        .map(|pairs| {
                            izip!(pairs)
                                .collect::<std::collections::HashMap<String, serde_json::Value>>()
                        })
                        .map(LoadAllSnapshotsResult::Ok)
                        .unwrap_or_else(LoadAllSnapshotsResult::Error);

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

    let builder = std::env::var("DYNA_S3_ENDPOINT")
        .ok()
        .map(|endpoint| {
            object_store::aws::AmazonS3Builder::from_env()
                .with_bucket_name(&bucket)
                .with_endpoint(&endpoint)
                .with_allow_http(true)
        })
        .unwrap_or_else(|| {
            object_store::aws::AmazonS3Builder::from_env()
                .with_bucket_name(&bucket)
        });

    builder.build().map(|s| Arc::new(s) as Arc<dyn ObjectStore>).map_err(Into::into)
}
