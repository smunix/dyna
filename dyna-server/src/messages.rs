//! Elfo message definitions for inter-actor communication.
//!
//! These messages define the typed protocol between the three actor groups:
//! - **API → Changeset**: `HandlePush`, `HandlePull`,
//!   `HandleClone`, `HandlePromote`, `HandleGetChangeset`, `HandleListChannels`.
//! - **Changeset → Storage**: `StoreChangeset`, `LoadChangeset`,
//!   `StoreChannel`, `LoadChannel`, `ListChannels`.
//!
//! All messages use elfo's `#[message]` macro for automatic serialization
//! and routing. The protocol is **changeset-centric**: push, pull, clone,
//! and promote operations exchange [`Changeset`] objects.

use dyna_core::models::{Changeset, Channel};
use dyna_core::protocol::{
    CloneResponse, CreateChannelResponse, ListChannelsResponse, PromoteResponse, PullResponse,
    PushResponse,
};
use elfo::prelude::*;

// ---------------------------------------------------------------------------
// Storage Actor Messages
// ---------------------------------------------------------------------------

/// Store a changeset in S3.
#[message(ret = StoreChangesetResult)]
pub struct StoreChangeset {
    pub changeset: Changeset,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StoreChangesetResult {
    Ok,
    Error(String),
}

/// Load a changeset from S3 by change_id.
#[message(ret = LoadChangesetResult)]
pub struct LoadChangeset {
    pub change_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LoadChangesetResult {
    Ok(Changeset),
    NotFound,
    Error(String),
}

/// Save a channel's metadata to S3.
#[message(ret = SaveChannelResult)]
pub struct SaveChannel {
    pub channel: Channel,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SaveChannelResult {
    Ok,
    Error(String),
}

/// Load a channel's metadata from S3.
#[message(ret = LoadChannelResult)]
pub struct LoadChannel {
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LoadChannelResult {
    Ok(Channel),
    NotFound,
    Error(String),
}

/// List all channels in S3.
#[message(ret = ListChannelsResult)]
pub struct ListAllChannels;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ListChannelsResult {
    Ok(Vec<Channel>),
    Error(String),
}

/// Save a resource snapshot to S3.
#[message(ret = SaveSnapshotResult)]
pub struct SaveSnapshot {
    pub resource_id: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SaveSnapshotResult {
    Ok,
    Error(String),
}

/// Load a resource snapshot from S3.
#[message(ret = LoadSnapshotResult)]
pub struct LoadSnapshot {
    pub resource_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LoadSnapshotResult {
    Ok(serde_json::Value),
    NotFound,
    Error(String),
}

/// Load all resource snapshots from S3.
#[message(ret = LoadAllSnapshotsResult)]
pub struct LoadAllSnapshots;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LoadAllSnapshotsResult {
    Ok(std::collections::HashMap<String, serde_json::Value>),
    Error(String),
}

// ---------------------------------------------------------------------------
// Changeset Messages
// ---------------------------------------------------------------------------

/// Handle a push request from a client (changeset-based).
#[message(ret = PushResponse)]
pub struct HandlePush {
    pub channel: String,
    pub changesets: Vec<Changeset>,
    pub expected_head: Option<String>,
}

/// Handle a pull request from a client (changeset-based).
#[message(ret = PullResponse)]
pub struct HandlePull {
    pub channel: String,
    pub since_change_id: Option<String>,
}

/// Handle a clone request from a client.
#[message(ret = CloneResponse)]
pub struct HandleClone {
    pub channel: Option<String>,
}

/// Handle a promote request (changeset-based).
#[message(ret = PromoteResponse)]
pub struct HandlePromote {
    pub source_channel: String,
    pub target_channel: String,
}

/// Handle a create channel request.
#[message(ret = CreateChannelResponse)]
pub struct HandleCreateChannel {
    pub name: String,
    pub fork_from: Option<String>,
}

/// Handle a list channels request.
#[message(ret = ListChannelsResponse)]
pub struct HandleListChannels;

/// Handle a get changeset detail request.
#[message(ret = dyna_core::protocol::GetChangesetResponse)]
pub struct HandleGetChangeset {
    pub change_id: String,
}
