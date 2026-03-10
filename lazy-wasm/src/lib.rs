//! # lazy-wasm
//!
//! WebAssembly lazy resource loader for Dyna — the browser counterpart of
//! `lazy-cat`.
//!
//! Provides a JavaScript-friendly API that lazily loads resources from a Dyna
//! server, keeping them in sync via WebSocket notifications.  All local state
//! lives in an in-memory VFS inside the browser.
//!
//! ## Usage from JavaScript
//!
//! ```js
//! import init, { LazyWasmClient } from './lazy_wasm.js';
//!
//! await init();
//! const client = await LazyWasmClient.connect("http://localhost:8080", "main");
//!
//! // List all known resource IDs
//! const ids = client.list_resources();   // string[]
//!
//! // Fetch a single resource
//! const value = client.get("acme.user.Alice");  // any (JSON object)
//!
//! // Stream all resources
//! client.for_each_all((id, value) => console.log(id, value));
//!
//! // Register a live-update callback
//! client.on_update((event) => {
//!     console.log("Update:", event);
//! });
//! ```

mod repository;
mod sync_client;

use anyhow::Result;
use dyna_core::notification::{
    ChangesetInfo, Notification, NotificationKind, NotificationPayload,
};
use dyna_core::protocol::{CloneRequest, PullRequest};
use itertools::izip;
use repository::Repository;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use sync_client::SyncClient;
use wasm_bindgen::prelude::*;

/// Serialize to JsValue using json_compatible mode so that
/// serde_json::Value::Object becomes a plain JS object (not an ES Map).
fn to_js_value<T: Serialize>(val: &T) -> Result<JsValue, JsError> {
    val.serialize(
        &serde_wasm_bindgen::Serializer::json_compatible(),
    )
    .map_err(|e| JsError::new(&e.to_string()))
}

// ---------------------------------------------------------------------------
// JS-serialisable event types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEvent {
    pub kind: String,
    pub timestamp: String,
    pub channel: String,
    pub changesets: Vec<ChangesetInfoJs>,
    pub new_head: Option<String>,
    pub affected_resource_ids: Vec<String>,
    pub updated_snapshots: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetInfoJs {
    pub change_id: String,
    pub message: String,
    pub author: String,
    pub patch_count: usize,
    pub affected_resources: Vec<String>,
}

impl From<&ChangesetInfo> for ChangesetInfoJs {
    fn from(ci: &ChangesetInfo) -> Self {
        Self {
            change_id: ci.change_id.clone(),
            message: ci.message.clone(),
            author: ci.author.clone(),
            patch_count: ci.patch_count,
            affected_resources: ci.affected_resources.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Internal shared state (single-threaded WASM — Rc<RefCell<…>>)
// ---------------------------------------------------------------------------

struct ClientState {
    repo: Repository,
    channel: String,
    loaded: HashSet<String>,
    known_ids: HashSet<String>,
    on_update_cb: Option<js_sys::Function>,
}

// ---------------------------------------------------------------------------
// LazyWasmClient — the main WASM-exported class
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct LazyWasmClient {
    state: Rc<RefCell<ClientState>>,
    server_url: String,
    #[allow(dead_code)]
    sync_client: SyncClient,
    _ws: Option<web_sys::WebSocket>,
}

#[wasm_bindgen]
impl LazyWasmClient {
    #[wasm_bindgen]
    pub async fn connect(server_url: &str, channel: &str) -> Result<LazyWasmClient, JsError> {
        console_error_panic_hook::set_once();

        let sync_client = SyncClient::new(server_url);
        let repo = Repository::new();
        repo.init().map_err(to_js)?;

        let clone_resp = sync_client
            .clone_repo(&CloneRequest {
                channel: Some(channel.to_string()),
            })
            .await
            .map_err(to_js)?;

        web_sys::console::log_1(
            &format!(
                "lazy-wasm: clone response: {} channels, {} changesets, {} snapshots",
                clone_resp.channels.len(),
                clone_resp.changesets.len(),
                clone_resp.snapshots.len()
            )
            .into(),
        );
        izip!(&clone_resp.channels).for_each(|ch| {
            web_sys::console::log_1(
                &format!(
                    "lazy-wasm: channel '{}': head={:?}, {} changesets",
                    ch.name,
                    ch.head_change_id,
                    ch.changesets.len()
                )
                .into(),
            );
        });

        izip!(&clone_resp.changesets)
            .try_for_each(|cs| repo.store_changeset(cs))
            .map_err(to_js)?;

        izip!(&clone_resp.channels)
            .try_for_each(|ch| repo.save_channel(ch))
            .map_err(to_js)?;

        repo.switch_channel(channel).map_err(to_js)?;

        let known_ids: HashSet<String> = izip!(&clone_resp.changesets)
            .flat_map(|cs| izip!(&cs.patches).map(|p| p.target_resource.clone()))
            .chain(clone_resp.snapshots.keys().cloned())
            .collect();

        web_sys::console::log_1(
            &format!("lazy-wasm: known_ids count: {}", known_ids.len()).into(),
        );

        izip!(&clone_resp.snapshots)
            .try_for_each(|(rid, val)| repo.save_snapshot(rid, val))
            .map_err(to_js)?;

        let loaded: HashSet<String> = clone_resp.snapshots.keys().cloned().collect();
        web_sys::console::log_1(
            &format!("lazy-wasm: loaded (from snapshots): {}", loaded.len()).into(),
        );

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
            .transpose()
            .map_err(to_js)?;

        let state = Rc::new(RefCell::new(ClientState {
            repo,
            channel: channel.to_string(),
            loaded,
            known_ids,
            on_update_cb: None,
        }));

        let ws = spawn_ws_listener(server_url, channel, Rc::clone(&state))?;

        Ok(Self {
            state,
            server_url: server_url.to_string(),
            sync_client,
            _ws: Some(ws),
        })
    }

    #[wasm_bindgen]
    pub fn list_resources(&self) -> Result<JsValue, JsError> {
        let st = self.state.borrow();
        let mut ids: Vec<&String> = st.known_ids.iter().collect();
        ids.sort();
        to_js_value(&ids)
    }

    #[wasm_bindgen]
    pub fn get(&self, resource_id: &str) -> Result<JsValue, JsError> {
        let mut st = self.state.borrow_mut();
        if !st.loaded.contains(resource_id) {
            materialise_resource(&st.repo, &st.channel, resource_id).map_err(to_js)?;
            st.loaded.insert(resource_id.to_string());
        }
        st.repo
            .load_snapshot(resource_id)
            .map_err(to_js)?
            .ok_or_else(|| JsError::new(&format!("Resource '{}' not found", resource_id)))
            .and_then(|v| to_js_value(&v))
    }

    #[wasm_bindgen]
    pub fn get_all(&self) -> Result<JsValue, JsError> {
        let mut st = self.state.borrow_mut();
        let ids: Vec<String> = st.known_ids.iter().cloned().collect();
        let to_load: HashSet<String> = izip!(&ids)
            .filter(|id| !st.loaded.contains(id.as_str()))
            .cloned()
            .collect();
        if !to_load.is_empty() {
            materialise_batch(&st.repo, &st.channel, &to_load).map_err(to_js)?;
            izip!(&to_load).for_each(|id| {
                st.loaded.insert(id.clone());
            });
        }
        let mut map: HashMap<String, Value> = HashMap::new();
        izip!(&ids).for_each(|id| {
            st.repo
                .load_snapshot(id)
                .ok()
                .flatten()
                .map(|v| map.insert(id.clone(), v));
        });
        to_js_value(&map)
    }

    #[wasm_bindgen]
    pub fn for_each_all(&self, callback: &js_sys::Function) -> Result<(), JsError> {
        let mut st = self.state.borrow_mut();
        let ids: Vec<String> = st.known_ids.iter().cloned().collect();
        let to_load: HashSet<String> = izip!(&ids)
            .filter(|id| !st.loaded.contains(id.as_str()))
            .cloned()
            .collect();
        if !to_load.is_empty() {
            materialise_batch(&st.repo, &st.channel, &to_load).map_err(to_js)?;
            izip!(&to_load).for_each(|id| {
                st.loaded.insert(id.clone());
            });
        }
        let this = JsValue::null();
        izip!(&ids).for_each(|id| {
            st.repo
                .load_snapshot(id)
                .ok()
                .flatten()
                .and_then(|v| to_js_value(&v).ok())
                .map(|js_val| {
                    let _ = callback.call2(&this, &JsValue::from_str(id), &js_val);
                });
        });
        Ok(())
    }

    #[wasm_bindgen]
    pub fn channel(&self) -> String {
        self.state.borrow().channel.clone()
    }

    #[wasm_bindgen]
    pub fn server_url(&self) -> String {
        self.server_url.clone()
    }

    #[wasm_bindgen]
    pub fn on_update(&self, callback: js_sys::Function) {
        self.state.borrow_mut().on_update_cb = Some(callback);
    }
}

// ---------------------------------------------------------------------------
// Materialisation helpers
// ---------------------------------------------------------------------------

fn materialise_resource(repo: &Repository, channel: &str, resource_id: &str) -> Result<()> {
    if repo.load_snapshot(resource_id)?.is_some() {
        web_sys::console::log_1(
            &format!("lazy-wasm: materialise_resource({resource_id}): already have snapshot").into(),
        );
        return Ok(());
    }
    let channel_data = repo.load_channel(channel)?;
    web_sys::console::log_1(
        &format!(
            "lazy-wasm: materialise_resource({resource_id}): channel '{}' has {} changesets",
            channel,
            channel_data.changesets.len()
        )
        .into(),
    );
    let mut patch_count = 0usize;
    let snapshot = izip!(&channel_data.changesets)
        .filter_map(|cid| repo.load_changeset(cid).ok())
        .flat_map(|cs| cs.patches.into_iter())
        .filter(|p| p.target_resource == resource_id)
        .try_fold(Value::Null, |acc, patch| -> Result<Value> {
            patch_count += 1;
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
    web_sys::console::log_1(
        &format!(
            "lazy-wasm: materialise_resource({resource_id}): found {patch_count} patches, snapshot is_null={}",
            snapshot.is_null()
        )
        .into(),
    );
    if !snapshot.is_null() {
        repo.save_snapshot(resource_id, &snapshot)?;
    }
    Ok(())
}

fn materialise_batch(
    repo: &Repository,
    channel: &str,
    resource_ids: &HashSet<String>,
) -> Result<()> {
    let channel_data = repo.load_channel(channel)?;
    let mut accumulators: HashMap<String, Value> = HashMap::with_capacity(resource_ids.len());
    izip!(&channel_data.changesets)
        .filter_map(|cid| repo.load_changeset(cid).ok())
        .for_each(|cs| {
            izip!(cs.patches).for_each(|patch| {
                if !resource_ids.contains(&patch.target_resource) {
                    return;
                }
                let acc = accumulators
                    .entry(patch.target_resource.clone())
                    .or_insert(Value::Null);
                patch
                    .result_snapshot
                    .map(|snap| { *acc = snap; })
                    .unwrap_or_else(|| {
                        if acc.is_null() {
                            *acc = serde_json::json!({});
                        }
                        let _ = dyna_core::diff::apply_patch(acc, &patch.operations);
                    });
            });
        });
    izip!(accumulators)
        .filter(|(_, v)| !v.is_null())
        .try_for_each(|(rid, snapshot)| repo.save_snapshot(&rid, &snapshot))
}

// ---------------------------------------------------------------------------
// WebSocket listener
// ---------------------------------------------------------------------------

fn spawn_ws_listener(
    server_url: &str,
    channel: &str,
    state: Rc<RefCell<ClientState>>,
) -> Result<web_sys::WebSocket, JsError> {
    let ws_url = server_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let ws_url = format!("{}/api/v1/ws", ws_url.trim_end_matches('/'));

    let ws =
        web_sys::WebSocket::new(&ws_url).map_err(|e| JsError::new(&format!("{:?}", e)))?;
    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

    let pull_url = server_url.to_string();
    let pull_channel = channel.to_string();
    let state_clone = Rc::clone(&state);

    let onmessage =
        Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let msg_str: String = event
                .data()
                .as_string()
                .or_else(|| {
                    js_sys::JSON::stringify(&event.data())
                        .ok()
                        .map(|s| s.into())
                })
                .unwrap_or_default();

            if msg_str.is_empty() {
                return;
            }

            let notification = match Notification::from_json(&msg_str) {
                Ok(n) => n,
                Err(_) => return,
            };

            let (notif_channel, changesets_info, new_head, affected_ids) =
                match &notification.payload {
                    NotificationPayload::Push(p) => (
                        p.channel.clone(),
                        p.changesets.clone(),
                        p.new_head.clone(),
                        p.changesets
                            .iter()
                            .flat_map(|cs| cs.affected_resources.iter().cloned())
                            .collect::<Vec<_>>(),
                    ),
                    NotificationPayload::Promotion(p) => (
                        p.target_channel.clone(),
                        p.promoted_changesets.clone(),
                        p.new_head.clone(),
                        p.promoted_changesets
                            .iter()
                            .flat_map(|cs| cs.affected_resources.iter().cloned())
                            .collect::<Vec<_>>(),
                    ),
                };

            {
                let st = state_clone.borrow();
                if notif_channel != st.channel {
                    return;
                }
            }

            let state_inner = Rc::clone(&state_clone);
            let pull_url_inner = pull_url.clone();
            let pull_channel_inner = pull_channel.clone();
            let kind_str = match notification.kind {
                NotificationKind::Push => "push".to_string(),
                NotificationKind::Promotion => "promotion".to_string(),
            };
            let timestamp = notification.timestamp.clone();
            let changesets_for_event = changesets_info;
            let new_head_for_event = new_head;
            let affected_for_event = affected_ids;

            wasm_bindgen_futures::spawn_local(async move {
                let client = SyncClient::new(&pull_url_inner);

                let known_head = state_inner
                    .borrow()
                    .repo
                    .load_sync_state()
                    .ok()
                    .and_then(|ss| ss.remote_heads.get(&pull_channel_inner).cloned());

                let pull_req = PullRequest {
                    channel: pull_channel_inner.clone(),
                    since_change_id: known_head,
                };

                let pull_result = client.pull(&pull_req).await;
                let mut updated_snapshots: HashMap<String, Value> = HashMap::new();

                if let Ok(resp) = pull_result {
                    let mut st = state_inner.borrow_mut();

                    izip!(&resp.changesets).for_each(|cs| {
                        let _ = st.repo.store_changeset(cs);
                    });

                    if let Ok(mut ch) = st.repo.load_channel(&pull_channel_inner) {
                        izip!(&resp.changesets).for_each(|cs| {
                            if !ch.changesets.contains(&cs.change_id) {
                                ch.changesets.push(cs.change_id.clone());
                            }
                        });
                        ch.head_change_id = resp.channel.head_change_id.clone();
                        let _ = st.repo.save_channel(&ch);
                    }

                    resp.channel.head_change_id.as_ref().map(|head| {
                        st.repo.load_sync_state().ok().map(|mut ss| {
                            ss.remote_heads
                                .insert(pull_channel_inner.clone(), head.clone());
                            let _ = st.repo.save_sync_state(&ss);
                        });
                    });

                    izip!(&affected_for_event).for_each(|rid| {
                        st.known_ids.insert(rid.clone());
                    });

                    // Materialise each affected resource
                    izip!(&affected_for_event).for_each(|rid| {
                        if let Ok(channel_data) = st.repo.load_channel(&st.channel) {
                            let snapshot = izip!(&channel_data.changesets)
                                .filter_map(|cid| st.repo.load_changeset(cid).ok())
                                .flat_map(|cs| cs.patches.into_iter())
                                .filter(|p| &p.target_resource == rid)
                                .fold(Value::Null, |acc, patch| {
                                    patch.result_snapshot.unwrap_or_else(|| {
                                        let mut current = if acc.is_null() {
                                            serde_json::json!({})
                                        } else {
                                            acc.clone()
                                        };
                                        let _ = dyna_core::diff::apply_patch(
                                            &mut current,
                                            &patch.operations,
                                        );
                                        current
                                    })
                                });
                            if !snapshot.is_null() {
                                let _ = st.repo.save_snapshot(rid, &snapshot);
                                st.loaded.insert(rid.clone());
                                updated_snapshots.insert(rid.clone(), snapshot);
                            }
                        }
                    });

                    let cb = st.on_update_cb.clone();
                    drop(st);

                    if let Some(callback) = cb {
                        let event = UpdateEvent {
                            kind: kind_str,
                            timestamp,
                            channel: pull_channel_inner,
                            changesets: changesets_for_event
                                .iter()
                                .map(ChangesetInfoJs::from)
                                .collect(),
                            new_head: new_head_for_event,
                            affected_resource_ids: affected_for_event,
                            updated_snapshots,
                        };
                        if let Ok(js_event) = to_js_value(&event) {
                            let _ = callback.call1(&JsValue::null(), &js_event);
                        }
                    }
                }
            });
        });

    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let onopen = Closure::<dyn Fn()>::new(|| {
        web_sys::console::log_1(&"lazy-wasm: WebSocket connected".into());
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let onerror = Closure::<dyn Fn(web_sys::ErrorEvent)>::new(|e: web_sys::ErrorEvent| {
        web_sys::console::error_1(
            &format!("lazy-wasm: WebSocket error: {:?}", e.message()).into(),
        );
    });
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    Ok(ws)
}

fn to_js(e: anyhow::Error) -> JsError {
    JsError::new(&e.to_string())
}
