//! API Actor — HTTP bridge between CLI clients and the elfo actor system.
//!
//! This actor bridges the HTTP layer (axum) with the elfo message-passing
//! system. It exposes the following REST endpoints:
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | `POST` | `/api/v1/push` | Push changesets to a channel |
//! | `POST` | `/api/v1/pull` | Pull changesets since a given ID |
//! | `POST` | `/api/v1/clone` | Clone all channels and changesets |
//! | `POST` | `/api/v1/promote` | Promote changesets between channels |
//! | `POST` | `/api/v1/channels` | Create a new channel |
//! | `GET`  | `/api/v1/channels` | List all channels |
//! | `GET`  | `/api/v1/changesets/:id` | Fetch a single changeset |
//!
//! ## Compression
//!
//! - **Requests**: Clients may send gzip-compressed bodies with
//!   `Content-Encoding: gzip`. The server transparently decompresses them
//!   via a custom middleware layer before JSON parsing.
//! - **Responses**: The server compresses all responses with gzip via
//!   `tower-http`'s `CompressionLayer` when the client sends
//!   `Accept-Encoding: gzip`.

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use dyna_core::compression;
use dyna_core::protocol::*;
use elfo::prelude::*;
use tokio::sync::{mpsc, oneshot};
use tower_http::compression::CompressionLayer;

use crate::messages::*;

/// The shared state for axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub request_tx: mpsc::Sender<ApiRequest>,
}

/// An API request forwarded from axum to the actor loop.
pub enum ApiRequest {
    Push {
        body: PushRequest,
        reply: oneshot::Sender<PushResponse>,
    },
    Pull {
        body: PullRequest,
        reply: oneshot::Sender<PullResponse>,
    },
    Clone {
        body: CloneRequest,
        reply: oneshot::Sender<CloneResponse>,
    },
    Promote {
        body: PromoteRequest,
        reply: oneshot::Sender<PromoteResponse>,
    },
    CreateChannel {
        body: CreateChannelRequest,
        reply: oneshot::Sender<CreateChannelResponse>,
    },
    ListChannels {
        reply: oneshot::Sender<ListChannelsResponse>,
    },
    GetChangeset {
        change_id: String,
        reply: oneshot::Sender<GetChangesetResponse>,
    },
    Health {
        reply: oneshot::Sender<HealthResponse>,
    },
}

/// Create the API actor blueprint.
pub fn new(bind_addr: String) -> Blueprint {
    ActorGroup::new().exec(move |mut ctx| {
        let bind_addr = bind_addr.clone();
        async move {
            tracing::info!(addr = %bind_addr, "API actor starting");

            let (request_tx, mut request_rx) = mpsc::channel::<ApiRequest>(256);

            let app = build_router(AppState {
                request_tx: request_tx.clone(),
            });

            tokio::net::TcpListener::bind(&bind_addr)
                .await
                .map(|listener| {
                    tracing::info!(addr = %bind_addr, "HTTP server listening");
                    tokio::spawn(async move {
                        axum::serve(listener, app).await.ok();
                    });
                })
                .expect("Failed to bind HTTP server");

            // Main actor loop: forward requests from axum to the changeset actor
            loop {
                tokio::select! {
                    envelope = ctx.recv() => {
                        if envelope.is_none() {
                            tracing::info!("API actor mailbox closed, shutting down");
                            break;
                        }
                    }

                    Some(request) = request_rx.recv() => {
                        dispatch_request(&ctx, request).await;
                    }
                }
            }
        }
    })
}

/// Dispatch an API request to the appropriate elfo message handler.
///
/// Each variant is handled by constructing the elfo message, resolving it,
/// and mapping the result through `and_then`/`unwrap_or_else` chains.
async fn dispatch_request(ctx: &Context, request: ApiRequest) {
    match request {
        ApiRequest::Push { body, reply } => {
            let response = ctx
                .request(HandlePush {
                    channel: body.channel,
                    changesets: body.changesets,
                    expected_head: body.expected_head,
                })
                .resolve()
                .await
                .unwrap_or_else(|e| PushResponse {
                    success: false,
                    new_head: None,
                    accepted_count: 0,
                    error: Some(format!("Internal error: {}", e)),
                });
            let _ = reply.send(response);
        }

        ApiRequest::Pull { body, reply } => {
            let response = ctx
                .request(HandlePull {
                    channel: body.channel,
                    since_change_id: body.since_change_id,
                })
                .resolve()
                .await
                .unwrap_or_else(|_| PullResponse {
                    changesets: vec![],
                    current_head: None,
                    channel: dyna_core::models::Channel::new("error"),
                });
            let _ = reply.send(response);
        }

        ApiRequest::Clone { body, reply } => {
            let response = ctx
                .request(HandleClone {
                    channel: body.channel,
                })
                .resolve()
                .await
                .unwrap_or_else(|_| CloneResponse {
                    channels: vec![],
                    changesets: vec![],
                    snapshots: std::collections::HashMap::new(),
                });
            let _ = reply.send(response);
        }

        ApiRequest::Promote { body, reply } => {
            let response = ctx
                .request(HandlePromote {
                    source_channel: body.source_channel,
                    target_channel: body.target_channel,
                })
                .resolve()
                .await
                .unwrap_or_else(|e| PromoteResponse {
                    success: false,
                    promoted_changesets: vec![],
                    new_head: None,
                    error: Some(format!("Internal error: {}", e)),
                });
            let _ = reply.send(response);
        }

        ApiRequest::CreateChannel { body, reply } => {
            let response = ctx
                .request(HandleCreateChannel {
                    name: body.name,
                    fork_from: body.fork_from,
                })
                .resolve()
                .await
                .unwrap_or_else(|e| CreateChannelResponse {
                    success: false,
                    channel: dyna_core::models::Channel::new("error"),
                    error: Some(format!("Internal error: {}", e)),
                });
            let _ = reply.send(response);
        }

        ApiRequest::ListChannels { reply } => {
            let response = ctx
                .request(HandleListChannels)
                .resolve()
                .await
                .unwrap_or_else(|_| ListChannelsResponse { channels: vec![] });
            let _ = reply.send(response);
        }

        ApiRequest::GetChangeset { change_id, reply } => {
            let response = ctx
                .request(HandleGetChangeset { change_id })
                .resolve()
                .await
                .unwrap_or_else(|e| GetChangesetResponse {
                    changeset: None,
                    error: Some(format!("Internal error: {}", e)),
                });
            let _ = reply.send(response);
        }

        ApiRequest::Health { reply } => {
            let _ = reply.send(HealthResponse {
                status: "ok".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                uptime_seconds: 0,
            });
        }
    }
}

/// Maximum request body size: 256 MiB.
///
/// Axum's default body limit is 2 MiB, which is too small for push requests
/// that contain large JSON resources with embedded snapshots. We raise it to
/// 256 MiB to accommodate bulk pushes. This can be overridden per-route if
/// needed via `DefaultBodyLimit::max()` on individual route layers.
const MAX_BODY_SIZE: usize = 256 * 1024 * 1024;

/// Middleware that transparently decompresses gzip-encoded request bodies.
///
/// If the incoming request has `Content-Encoding: gzip`, the body is read,
/// decompressed, and replaced before passing to the next handler. This allows
/// axum's `Json<T>` extractor to work normally on the decompressed data.
async fn decompress_request_body(request: Request, next: Next) -> Response {
    let has_gzip = request
        .headers()
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("gzip"))
        .unwrap_or(false);

    if !has_gzip {
        return next.run(request).await;
    }

    let (mut parts, body) = request.into_parts();

    // Read the full body using axum's body-to-bytes
    let body_bytes = match axum::body::to_bytes(body, MAX_BODY_SIZE).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!(error = %e, "Failed to read compressed request body");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Failed to read request body"})),
            )
                .into_response();
        }
    };

    // Decompress
    let decompressed = match compression::decompress(&body_bytes) {
        Ok(data) => data,
        Err(e) => {
            tracing::error!(error = %e, "Failed to decompress gzip request body");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Failed to decompress gzip body"})),
            )
                .into_response();
        }
    };

    // Remove Content-Encoding header and update Content-Length
    parts.headers.remove(header::CONTENT_ENCODING);
    parts
        .headers
        .insert(header::CONTENT_LENGTH, decompressed.len().into());

    let new_request = Request::from_parts(parts, Body::from(decompressed));
    next.run(new_request).await
}

/// Build the axum router with all API routes.
///
/// The router includes:
/// - `decompress_request_body` middleware for transparent gzip request decompression
/// - `CompressionLayer` from tower-http for automatic gzip response compression
/// - `DefaultBodyLimit` of 256 MiB for large payloads
fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/push", post(push_handler))
        .route("/api/v1/pull", post(pull_handler))
        .route("/api/v1/clone", post(clone_handler))
        .route("/api/v1/promote", post(promote_handler))
        .route("/api/v1/channels", get(list_channels_handler))
        .route("/api/v1/channels", post(create_channel_handler))
        .route("/api/v1/changesets/:change_id", get(get_changeset_handler))
        .layer(middleware::from_fn(decompress_request_body))
        .layer(CompressionLayer::new().gzip(true))
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Generic handler helper
// ---------------------------------------------------------------------------

/// Send an API request through the mpsc channel and await the oneshot response.
/// Maps send-failure and recv-failure into a typed HTTP error response.
async fn send_and_recv<R: serde::Serialize>(
    state: &AppState,
    make_request: impl FnOnce(oneshot::Sender<R>) -> ApiRequest,
) -> Result<R, (StatusCode, Json<serde_json::Value>)> {
    let (reply_tx, reply_rx) = oneshot::channel();
    state
        .request_tx
        .send(make_request(reply_tx))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::to_value(ErrorResponse::new("Server unavailable", "UNAVAILABLE")).unwrap()),
            )
        })?;

    reply_rx.await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Request timeout", "TIMEOUT")).unwrap()),
        )
    })
}

/// Convert a serializable response into an axum JSON response with the given status.
fn json_response<R: serde::Serialize>(
    status: StatusCode,
    response: R,
) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::to_value(response).unwrap()))
}

// ---------------------------------------------------------------------------
// Axum Handlers — each uses send_and_recv + functional mapping
// ---------------------------------------------------------------------------

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    send_and_recv(&state, |reply| ApiRequest::Health { reply })
        .await
        .map(|r| json_response(StatusCode::OK, r))
        .unwrap_or_else(|e| e)
}

async fn push_handler(
    State(state): State<AppState>,
    Json(body): Json<PushRequest>,
) -> impl IntoResponse {
    send_and_recv(&state, |reply| ApiRequest::Push { body, reply })
        .await
        .map(|r| {
            r.success
                .then(|| json_response(StatusCode::OK, &r))
                .unwrap_or_else(|| json_response(StatusCode::CONFLICT, &r))
        })
        .unwrap_or_else(|e| e)
}

async fn pull_handler(
    State(state): State<AppState>,
    Json(body): Json<PullRequest>,
) -> impl IntoResponse {
    send_and_recv(&state, |reply| ApiRequest::Pull { body, reply })
        .await
        .map(|r| json_response(StatusCode::OK, r))
        .unwrap_or_else(|e| e)
}

async fn clone_handler(
    State(state): State<AppState>,
    Json(body): Json<CloneRequest>,
) -> impl IntoResponse {
    send_and_recv(&state, |reply| ApiRequest::Clone { body, reply })
        .await
        .map(|r| json_response(StatusCode::OK, r))
        .unwrap_or_else(|e| e)
}

async fn promote_handler(
    State(state): State<AppState>,
    Json(body): Json<PromoteRequest>,
) -> impl IntoResponse {
    send_and_recv(&state, |reply| ApiRequest::Promote { body, reply })
        .await
        .map(|r| {
            r.success
                .then(|| json_response(StatusCode::OK, &r))
                .unwrap_or_else(|| json_response(StatusCode::CONFLICT, &r))
        })
        .unwrap_or_else(|e| e)
}

async fn list_channels_handler(State(state): State<AppState>) -> impl IntoResponse {
    send_and_recv(&state, |reply| ApiRequest::ListChannels { reply })
        .await
        .map(|r| json_response(StatusCode::OK, r))
        .unwrap_or_else(|e| e)
}

async fn create_channel_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateChannelRequest>,
) -> impl IntoResponse {
    send_and_recv(&state, |reply| ApiRequest::CreateChannel { body, reply })
        .await
        .map(|r| {
            r.success
                .then(|| json_response(StatusCode::CREATED, &r))
                .unwrap_or_else(|| json_response(StatusCode::CONFLICT, &r))
        })
        .unwrap_or_else(|e| e)
}

async fn get_changeset_handler(
    State(state): State<AppState>,
    Path(change_id): Path<String>,
) -> impl IntoResponse {
    send_and_recv(&state, |reply| ApiRequest::GetChangeset {
        change_id,
        reply,
    })
    .await
    .map(|r| {
        r.changeset
            .as_ref()
            .map(|_| json_response(StatusCode::OK, &r))
            .unwrap_or_else(|| json_response(StatusCode::NOT_FOUND, &r))
    })
    .unwrap_or_else(|e| e)
}
