//! HTTP API protocol types shared between the CLI client and the remote server.
//!
//! These types define the request and response bodies for the REST API used
//! for synchronization between the CLI and the server.

use crate::models::{Channel, Patch};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Push / Pull
// ---------------------------------------------------------------------------

/// Request to push patches to the remote server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    /// The channel to push to.
    pub channel: String,
    /// The patches to push, in dependency order.
    pub patches: Vec<Patch>,
    /// The expected current head of the channel (for optimistic concurrency).
    pub expected_head: Option<String>,
}

/// Response from a push operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    pub success: bool,
    /// The new head of the channel after the push.
    pub new_head: Option<String>,
    /// Number of patches accepted.
    pub accepted_count: usize,
    /// Error message if the push failed.
    pub error: Option<String>,
}

/// Request to pull patches from the remote server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    /// The channel to pull from.
    pub channel: String,
    /// The hash of the last patch the client has for this channel.
    /// If None, the client wants all patches.
    pub since_hash: Option<String>,
}

/// Response from a pull operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    /// The patches that the client is missing, in dependency order.
    pub patches: Vec<Patch>,
    /// The current head of the channel on the remote.
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
    /// Optional: specific channel to clone. Defaults to "main".
    pub channel: Option<String>,
}

/// Response from a clone operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneResponse {
    /// All channels in the repository.
    pub channels: Vec<Channel>,
    /// All patches in the repository, in dependency order.
    pub patches: Vec<Patch>,
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
    /// Optional: fork from this channel (copy its patches).
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
// Promote
// ---------------------------------------------------------------------------

/// Request to promote patches from one channel to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteRequest {
    /// Source channel to promote from.
    pub source_channel: String,
    /// Target channel to promote to (usually "main").
    pub target_channel: String,
}

/// Response from a promote operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteResponse {
    pub success: bool,
    /// Hashes of the promoted patches.
    pub promoted_patches: Vec<String>,
    /// The new head of the target channel.
    pub new_head: Option<String>,
    pub error: Option<String>,
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
