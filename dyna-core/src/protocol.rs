//! HTTP API protocol types shared between the CLI client and the remote server.
//!
//! These types define the request and response bodies for the REST API.
//! The API is **changeset-centric**: all push, pull, clone, and promote
//! operations exchange [`crate::models::Changeset`] objects rather than
//! individual patches.
//!
//! Key endpoints:
//! - `POST /api/v1/push` — Push `Vec<Changeset>` to a channel.
//! - `POST /api/v1/pull` — Pull changesets since a given `since_change_id`.
//! - `POST /api/v1/clone` — Clone all channels and changesets.
//! - `POST /api/v1/promote` — Promote changesets between channels.
//! - `GET /api/v1/changesets/:change_id` — Fetch a single changeset by ID.

use crate::models::{Changeset, Channel};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Push / Pull (changeset-based)
// ---------------------------------------------------------------------------

/// Request to push changesets to the remote server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    /// The channel to push to.
    pub channel: String,
    /// The changesets to push, in dependency order.
    pub changesets: Vec<Changeset>,
    /// The expected current head change_id of the channel (optimistic concurrency).
    pub expected_head: Option<String>,
}

/// Response from a push operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    pub success: bool,
    /// The new head change_id of the channel after the push.
    pub new_head: Option<String>,
    /// Number of changesets accepted.
    pub accepted_count: usize,
    /// Error message if the push failed.
    pub error: Option<String>,
}

/// Request to pull changesets from the remote server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    /// The channel to pull from.
    pub channel: String,
    /// The change_id of the last changeset the client has for this channel.
    pub since_change_id: Option<String>,
}

/// Response from a pull operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    /// The changesets that the client is missing, in dependency order.
    pub changesets: Vec<Changeset>,
    /// The current head change_id of the channel on the remote.
    pub current_head: Option<String>,
    /// The channel metadata.
    pub channel: Channel,
}

// ---------------------------------------------------------------------------
// Clone
// ---------------------------------------------------------------------------

/// Request to clone a repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneRequest {
    pub channel: Option<String>,
}

/// Response from a clone operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneResponse {
    /// All channels in the repository.
    pub channels: Vec<Channel>,
    /// All changesets in the repository, in dependency order.
    pub changesets: Vec<Changeset>,
    /// Current resource snapshots (resource_id -> JSON value).
    pub snapshots: std::collections::HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Channel management
// ---------------------------------------------------------------------------

/// Request to list all channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListChannelsRequest {}

/// Response listing all channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListChannelsResponse {
    pub channels: Vec<Channel>,
}

/// Request to create a new channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub fork_from: Option<String>,
}

/// Response from creating a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChannelResponse {
    pub success: bool,
    pub channel: Channel,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Delete Channel
// ---------------------------------------------------------------------------

/// Request to delete a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteChannelRequest {
    /// The channel name to delete.
    pub channel: String,
    /// Force deletion even if the channel has not been promoted to main.
    pub force: bool,
}

/// Response from a delete channel operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteChannelResponse {
    pub success: bool,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Promote
// ---------------------------------------------------------------------------

/// Request to promote changesets from one channel to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteRequest {
    pub source_channel: String,
    pub target_channel: String,
}

/// Response from a promote operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteResponse {
    pub success: bool,
    /// Change IDs of the promoted changesets.
    pub promoted_changesets: Vec<String>,
    /// Detailed info for each promoted changeset (for notifications).
    #[serde(default)]
    pub changeset_infos: Vec<crate::notification::ChangesetInfo>,
    /// The new head change_id of the target channel.
    pub new_head: Option<String>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Changeset detail query
// ---------------------------------------------------------------------------

/// Request to get details of a specific changeset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetChangesetRequest {
    pub change_id: String,
}

/// Response with changeset details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetChangesetResponse {
    pub changeset: Option<Changeset>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Resource History
// ---------------------------------------------------------------------------

/// Response from a resource history query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceHistoryResponse {
    /// The resource_id that was queried.
    pub resource_id: String,
    /// History entries in reverse chronological order.
    pub entries: Vec<ResourceHistoryEntry>,
    /// Error message if the query failed.
    pub error: Option<String>,
}

/// A single entry in a resource's change history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceHistoryEntry {
    /// The changeset's change_id.
    pub change_id: String,
    /// The changeset's commit_hash.
    pub commit_hash: String,
    /// The commit message.
    pub message: String,
    /// The author.
    pub author: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// The channel this changeset belongs to.
    pub channel: String,
    /// The operations applied to this resource in this changeset.
    pub operations: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Status / Health
// ---------------------------------------------------------------------------

/// Server health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
}

/// Error response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: String,
}

impl ErrorResponse {
    pub fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: code.into(),
        }
    }
}
