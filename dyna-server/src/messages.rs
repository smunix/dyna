//! Elfo message definitions for inter-actor communication.
//!
//! These messages define the protocol between the API Gateway actor,
//! the Changeset Manager actor, and the S3 Storage actor.
//!
//! Note: elfo's `#[message]` macro automatically derives Serialize and
//! Deserialize, so we must not derive them again.

use dyna_common::models::{Channel, Patch};
use dyna_common::protocol::{
    CloneResponse, CreateChannelResponse, ListChannelsResponse, PromoteResponse, PullResponse,
    PushResponse,
};
use elfo::prelude::*;

// ---------------------------------------------------------------------------
// Storage Actor Messages
// ---------------------------------------------------------------------------

/// Store a patch in S3.
#[message(ret = StorePatchResult)]
pub struct StorePatch {
    pub patch: Patch,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StorePatchResult {
    Ok,
    Error(String),
}

/// Load a patch from S3 by hash.
#[message(ret = LoadPatchResult)]
pub struct LoadPatch {
    pub hash: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LoadPatchResult {
    Ok(Patch),
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
// Changeset Manager Messages
// ---------------------------------------------------------------------------

/// Handle a push request from a client.
#[message(ret = PushResponse)]
pub struct HandlePush {
    pub channel: String,
    pub patches: Vec<Patch>,
    pub expected_head: Option<String>,
}

/// Handle a pull request from a client.
#[message(ret = PullResponse)]
pub struct HandlePull {
    pub channel: String,
    pub since_hash: Option<String>,
}

/// Handle a clone request from a client.
#[message(ret = CloneResponse)]
pub struct HandleClone {
    pub channel: Option<String>,
}

/// Handle a promote request.
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
