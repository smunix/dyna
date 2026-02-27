//! API Gateway Actor.
//!
//! This actor bridges the HTTP layer (axum) with the elfo actor system.
//! It receives HTTP requests via axum, translates them into elfo messages,
//! sends them to the Changeset Manager actor, and returns the responses.
//!
//! The protocol is now changeset-centric.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use dyna_common::protocol::*;
use elfo::prelude::*;
use tokio::sync::{mpsc, oneshot};

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

/// Create the API Gateway actor blueprint.
pub fn new(bind_addr: String) -> Blueprint {
    ActorGroup::new().exec(move |mut ctx| {
        let bind_addr = bind_addr.clone();
        async move {
            tracing::info!(addr = %bind_addr, "API Gateway actor starting");

            let (request_tx, mut request_rx) = mpsc::channel::<ApiRequest>(256);

            let state = AppState {
                request_tx: request_tx.clone(),
            };

            let app = build_router(state);

            let listener = tokio::net::TcpListener::bind(&bind_addr)
                .await
                .expect("Failed to bind HTTP server");

            tracing::info!(addr = %bind_addr, "HTTP server listening");

            tokio::spawn(async move {
                axum::serve(listener, app).await.ok();
            });

            // Main actor loop: forward requests from axum to the changeset manager
            loop {
                tokio::select! {
                    envelope = ctx.recv() => {
                        match envelope {
                            Some(_envelope) => {}
                            None => {
                                tracing::info!("API Gateway actor mailbox closed, shutting down");
                                break;
                            }
                        }
                    }

                    Some(request) = request_rx.recv() => {
                        match request {
                            ApiRequest::Push { body, reply } => {
                                let result = ctx.request(HandlePush {
                                    channel: body.channel,
                                    changesets: body.changesets,
                                    expected_head: body.expected_head,
                                }).resolve().await;

                                let response = match result {
                                    Ok(r) => r,
                                    Err(e) => PushResponse {
                                        success: false,
                                        new_head: None,
                                        accepted_count: 0,
                                        error: Some(format!("Internal error: {}", e)),
                                    },
                                };
                                let _ = reply.send(response);
                            }

                            ApiRequest::Pull { body, reply } => {
                                let result = ctx.request(HandlePull {
                                    channel: body.channel,
                                    since_change_id: body.since_change_id,
                                }).resolve().await;

                                let response = match result {
                                    Ok(r) => r,
                                    Err(_) => PullResponse {
                                        changesets: vec![],
                                        current_head: None,
                                        channel: dyna_common::models::Channel::new("error"),
                                    },
                                };
                                let _ = reply.send(response);
                            }

                            ApiRequest::Clone { body, reply } => {
                                let result = ctx.request(HandleClone {
                                    channel: body.channel,
                                }).resolve().await;

                                let response = match result {
                                    Ok(r) => r,
                                    Err(_) => CloneResponse {
                                        channels: vec![],
                                        changesets: vec![],
                                        snapshots: std::collections::HashMap::new(),
                                    },
                                };
                                let _ = reply.send(response);
                            }

                            ApiRequest::Promote { body, reply } => {
                                let result = ctx.request(HandlePromote {
                                    source_channel: body.source_channel,
                                    target_channel: body.target_channel,
                                }).resolve().await;

                                let response = match result {
                                    Ok(r) => r,
                                    Err(e) => PromoteResponse {
                                        success: false,
                                        promoted_changesets: vec![],
                                        new_head: None,
                                        error: Some(format!("Internal error: {}", e)),
                                    },
                                };
                                let _ = reply.send(response);
                            }

                            ApiRequest::CreateChannel { body, reply } => {
                                let result = ctx.request(HandleCreateChannel {
                                    name: body.name,
                                    fork_from: body.fork_from,
                                }).resolve().await;

                                let response = match result {
                                    Ok(r) => r,
                                    Err(e) => CreateChannelResponse {
                                        success: false,
                                        channel: dyna_common::models::Channel::new("error"),
                                        error: Some(format!("Internal error: {}", e)),
                                    },
                                };
                                let _ = reply.send(response);
                            }

                            ApiRequest::ListChannels { reply } => {
                                let result = ctx.request(HandleListChannels).resolve().await;

                                let response = match result {
                                    Ok(r) => r,
                                    Err(_) => ListChannelsResponse { channels: vec![] },
                                };
                                let _ = reply.send(response);
                            }

                            ApiRequest::GetChangeset { change_id, reply } => {
                                let result = ctx.request(HandleGetChangeset {
                                    change_id,
                                }).resolve().await;

                                let response = match result {
                                    Ok(r) => r,
                                    Err(e) => GetChangesetResponse {
                                        changeset: None,
                                        error: Some(format!("Internal error: {}", e)),
                                    },
                                };
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
                }
            }
        }
    })
}

/// Build the axum router with all API routes.
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
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Axum Handlers
// ---------------------------------------------------------------------------

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state
        .request_tx
        .send(ApiRequest::Health { reply: reply_tx })
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Server unavailable"})),
        );
    }
    match reply_rx.await {
        Ok(response) => (StatusCode::OK, Json(serde_json::to_value(response).unwrap())),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Request timeout"})),
        ),
    }
}

async fn push_handler(
    State(state): State<AppState>,
    Json(body): Json<PushRequest>,
) -> impl IntoResponse {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state
        .request_tx
        .send(ApiRequest::Push {
            body,
            reply: reply_tx,
        })
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Server unavailable", "UNAVAILABLE")).unwrap()),
        );
    }
    match reply_rx.await {
        Ok(response) => {
            let status = if response.success {
                StatusCode::OK
            } else {
                StatusCode::CONFLICT
            };
            (status, Json(serde_json::to_value(response).unwrap()))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Request timeout", "TIMEOUT")).unwrap()),
        ),
    }
}

async fn pull_handler(
    State(state): State<AppState>,
    Json(body): Json<PullRequest>,
) -> impl IntoResponse {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state
        .request_tx
        .send(ApiRequest::Pull {
            body,
            reply: reply_tx,
        })
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Server unavailable", "UNAVAILABLE")).unwrap()),
        );
    }
    match reply_rx.await {
        Ok(response) => (StatusCode::OK, Json(serde_json::to_value(response).unwrap())),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Request timeout", "TIMEOUT")).unwrap()),
        ),
    }
}

async fn clone_handler(
    State(state): State<AppState>,
    Json(body): Json<CloneRequest>,
) -> impl IntoResponse {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state
        .request_tx
        .send(ApiRequest::Clone {
            body,
            reply: reply_tx,
        })
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Server unavailable", "UNAVAILABLE")).unwrap()),
        );
    }
    match reply_rx.await {
        Ok(response) => (StatusCode::OK, Json(serde_json::to_value(response).unwrap())),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Request timeout", "TIMEOUT")).unwrap()),
        ),
    }
}

async fn promote_handler(
    State(state): State<AppState>,
    Json(body): Json<PromoteRequest>,
) -> impl IntoResponse {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state
        .request_tx
        .send(ApiRequest::Promote {
            body,
            reply: reply_tx,
        })
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Server unavailable", "UNAVAILABLE")).unwrap()),
        );
    }
    match reply_rx.await {
        Ok(response) => {
            let status = if response.success {
                StatusCode::OK
            } else {
                StatusCode::CONFLICT
            };
            (status, Json(serde_json::to_value(response).unwrap()))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Request timeout", "TIMEOUT")).unwrap()),
        ),
    }
}

async fn list_channels_handler(State(state): State<AppState>) -> impl IntoResponse {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state
        .request_tx
        .send(ApiRequest::ListChannels { reply: reply_tx })
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Server unavailable", "UNAVAILABLE")).unwrap()),
        );
    }
    match reply_rx.await {
        Ok(response) => (StatusCode::OK, Json(serde_json::to_value(response).unwrap())),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Request timeout", "TIMEOUT")).unwrap()),
        ),
    }
}

async fn create_channel_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateChannelRequest>,
) -> impl IntoResponse {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state
        .request_tx
        .send(ApiRequest::CreateChannel {
            body,
            reply: reply_tx,
        })
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Server unavailable", "UNAVAILABLE")).unwrap()),
        );
    }
    match reply_rx.await {
        Ok(response) => {
            let status = if response.success {
                StatusCode::CREATED
            } else {
                StatusCode::CONFLICT
            };
            (status, Json(serde_json::to_value(response).unwrap()))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Request timeout", "TIMEOUT")).unwrap()),
        ),
    }
}

async fn get_changeset_handler(
    State(state): State<AppState>,
    Path(change_id): Path<String>,
) -> impl IntoResponse {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state
        .request_tx
        .send(ApiRequest::GetChangeset {
            change_id,
            reply: reply_tx,
        })
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Server unavailable", "UNAVAILABLE")).unwrap()),
        );
    }
    match reply_rx.await {
        Ok(response) => {
            let status = if response.changeset.is_some() {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            };
            (status, Json(serde_json::to_value(response).unwrap()))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ErrorResponse::new("Request timeout", "TIMEOUT")).unwrap()),
        ),
    }
}
