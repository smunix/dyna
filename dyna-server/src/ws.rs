//! WebSocket notification hub for real-time event broadcasting.
//!
//! This module provides a [`NotificationHub`] that manages WebSocket connections
//! and broadcasts [`Notification`] events to all connected clients. The hub is
//! shared across axum handlers via `Arc`.
//!
//! ## Architecture
//!
//! ```text
//! promote_handler ──► NotificationHub::broadcast()
//!                          │
//!                          ├──► ws_client_1 (text frame)
//!                          ├──► ws_client_2 (text frame)
//!                          └──► ws_client_N (text frame)
//! ```
//!
//! Clients connect via `GET /api/v1/ws` and receive JSON-encoded notifications
//! as text frames. The connection is kept alive with periodic ping/pong frames.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use dyna_core::notification::Notification;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast;

/// The capacity of the broadcast channel. Late subscribers will miss messages
/// that overflow this buffer.
const BROADCAST_CAPACITY: usize = 256;

/// A hub that manages WebSocket notification broadcasting.
///
/// Internally uses a `tokio::sync::broadcast` channel. Each new WebSocket
/// connection subscribes to the broadcast channel and forwards messages.
#[derive(Clone)]
pub struct NotificationHub {
    sender: broadcast::Sender<String>,
}

impl NotificationHub {
    /// Create a new notification hub.
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self { sender }
    }

    /// Broadcast a notification to all connected WebSocket clients.
    ///
    /// Returns the number of clients that received the message.
    /// If no clients are connected, this is a no-op.
    pub fn broadcast(&self, notification: &Notification) -> usize {
        notification
            .to_json()
            .map(|json| self.sender.send(json).unwrap_or(0))
            .unwrap_or(0)
    }

    /// Subscribe to the broadcast channel.
    fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }
}

/// Axum handler for WebSocket upgrade at `GET /api/v1/ws`.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(hub): State<Arc<NotificationHub>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, hub))
}

/// Handle an individual WebSocket connection.
///
/// Subscribes to the notification hub and forwards all broadcast messages
/// to the client as text frames. Client messages are ignored (this is a
/// server-push-only channel). The connection is closed when the client
/// disconnects or an error occurs.
async fn handle_ws_connection(socket: WebSocket, hub: Arc<NotificationHub>) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let mut rx = hub.subscribe();

    // Spawn a task to forward broadcast messages to the WebSocket client
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if ws_sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Drain incoming messages (we don't process client messages, but we
    // need to consume them to detect disconnection)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(_msg)) = ws_receiver.next().await {
            // Ignore client messages; this is a server-push channel.
        }
    });

    // Wait for either task to complete (client disconnect or send error)
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    tracing::debug!("WebSocket client disconnected");
}
