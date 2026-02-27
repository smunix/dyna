//! S3 Storage Actor — persistent object storage for the Dyna server.
//!
//! This elfo actor encapsulates all interactions with the S3-compatible object
//! store via the `object_store` crate. It handles storing and retrieving:
//! - **Changesets** under `changesets/<commit_hash>.json`
//! - **Channels** under `channels/<name>.json`
//! - **Snapshots** under `snapshots/<resource_id>.json`
//!
//! In development mode, an in-memory store is used. In production, configure
//! `AmazonS3Builder::from_env()` for real S3 access.

use bytes::Bytes;
use itertools::izip;
use elfo::prelude::*;
use object_store::{ObjectStore, memory::InMemory, path::Path as ObjPath};
use std::sync::Arc;

use crate::messages::*;

/// Helper: serialize + put to object store, returning a typed result.
async fn store_json<T: serde::Serialize>(
    store: &dyn ObjectStore,
    path: &ObjPath,
    value: &T,
) -> Result<(), String> {
    serde_json::to_vec_pretty(value)
        .map_err(|e| e.to_string())
        .map(Bytes::from)
        .map(|bytes| (path.clone(), bytes))
        .map_err(|e| e.to_string())?;

    let data = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    store
        .put(path, Bytes::from(data).into())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Helper: get from object store + deserialize, returning Option or error.
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
                serde_json::from_slice(&data)
                    .map_err(|e| format!("Deserialization error: {}", e))
            })
            .map(Some),
        Err(object_store::Error::NotFound { .. }) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Helper: list objects under a prefix, load each, and deserialize.
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
            .trim_end_matches(".json")
            .to_string();

        let loaded = store
            .get(&obj.location)
            .await
            .ok()
            .and_then(|gr| futures::executor::block_on(gr.bytes()).ok())
            .and_then(|data| serde_json::from_slice::<T>(&data).ok())
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
            tracing::info!("S3 Storage actor started");

            while let Some(envelope) = ctx.recv().await {
                msg!(match envelope {
                    // ----------------------------------------------------------
                    // Changeset operations
                    // ----------------------------------------------------------
                    (StoreChangeset { changeset }, token) => {
                        let path = ObjPath::from(format!(
                            "changesets/{}.json",
                            changeset.change_id
                        ));
                        let result = store_json(store.as_ref(), &path, &changeset)
                            .await
                            .map(|()| {
                                tracing::debug!(change_id = %changeset.change_id, "Stored changeset");
                                StoreChangesetResult::Ok
                            })
                            .unwrap_or_else(|e| {
                                tracing::error!(error = %e, "Failed to store changeset");
                                StoreChangesetResult::Error(e)
                            });
                        ctx.respond(token, result);
                    }

                    (LoadChangeset { change_id }, token) => {
                        let path = ObjPath::from(format!("changesets/{}.json", change_id));
                        let result = load_json(store.as_ref(), &path)
                            .await
                            .map(|opt| {
                                opt.map(LoadChangesetResult::Ok)
                                    .unwrap_or(LoadChangesetResult::NotFound)
                            })
                            .unwrap_or_else(LoadChangesetResult::Error);
                        ctx.respond(token, result);
                    }

                    // ----------------------------------------------------------
                    // Channel operations
                    // ----------------------------------------------------------
                    (SaveChannel { channel }, token) => {
                        let path = ObjPath::from(format!("channels/{}.json", channel.name));
                        let result = store_json(store.as_ref(), &path, &channel)
                            .await
                            .map(|()| {
                                tracing::debug!(name = %channel.name, "Saved channel");
                                SaveChannelResult::Ok
                            })
                            .unwrap_or_else(SaveChannelResult::Error);
                        ctx.respond(token, result);
                    }

                    (LoadChannel { name }, token) => {
                        let path = ObjPath::from(format!("channels/{}.json", name));
                        let result = load_json(store.as_ref(), &path)
                            .await
                            .map(|opt| {
                                opt.map(LoadChannelResult::Ok)
                                    .unwrap_or(LoadChannelResult::NotFound)
                            })
                            .unwrap_or_else(LoadChannelResult::Error);
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
                    (SaveSnapshot { resource_id, value }, token) => {
                        let path = ObjPath::from(format!("snapshots/{}.json", resource_id));
                        let result = store_json(store.as_ref(), &path, &value)
                            .await
                            .map(|()| {
                                tracing::debug!(resource_id = %resource_id, "Saved snapshot");
                                SaveSnapshotResult::Ok
                            })
                            .unwrap_or_else(SaveSnapshotResult::Error);
                        ctx.respond(token, result);
                    }

                    (LoadSnapshot { resource_id }, token) => {
                        let path = ObjPath::from(format!("snapshots/{}.json", resource_id));
                        let result = load_json(store.as_ref(), &path)
                            .await
                            .map(|opt| {
                                opt.map(LoadSnapshotResult::Ok)
                                    .unwrap_or(LoadSnapshotResult::NotFound)
                            })
                            .unwrap_or_else(LoadSnapshotResult::Error);
                        ctx.respond(token, result);
                    }

                    (LoadAllSnapshots, token) => {
                        let prefix = ObjPath::from("snapshots/");
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
