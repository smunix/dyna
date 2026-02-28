//! HTTP sync client for communicating with the remote Dyna server.
//!
//! This module encapsulates all network interactions between the CLI and the
//! remote server, providing a clean async API for changeset-based push, pull,
//! clone, promote, and channel management operations.
//!
//! All request bodies are gzip-compressed and sent with `Content-Encoding: gzip`.
//! The client also sends `Accept-Encoding: gzip` so the server can compress
//! responses. Responses are transparently decompressed (reqwest handles this
//! automatically when the feature is enabled, but we also handle it manually
//! for robustness).

use anyhow::{Context, Result};
use dyna_core::compression;
use dyna_core::protocol::*;
use reqwest::Client;

/// Client for interacting with the Dyna remote server.
pub struct SyncClient {
    client: Client,
    base_url: String,
}

impl SyncClient {
    /// Create a new sync client for the given remote URL.
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Generic helper: POST gzip-compressed JSON, check status, parse response.
    ///
    /// The request body is serialized to JSON, gzip-compressed, and sent with
    /// `Content-Encoding: gzip` and `Content-Type: application/json` headers.
    /// `Accept-Encoding: gzip` is also sent to request compressed responses.
    async fn post_json<Req, Resp>(
        &self,
        endpoint: &str,
        request: &Req,
        operation: &str,
    ) -> Result<Resp>
    where
        Req: serde::Serialize,
        Resp: serde::de::DeserializeOwned,
    {
        let url = format!("{}{}", self.base_url, endpoint);
        let compressed_body = compression::compress_json(request)
            .context(format!("Failed to compress {} request body", operation))?;

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "gzip")
            .header("Accept-Encoding", "gzip")
            .body(compressed_body)
            .send()
            .await
            .context(format!(
                "Failed to connect to remote server for {}",
                operation
            ))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("{} failed (HTTP {}): {}", operation, status, body);
        }

        // Read response bytes and transparently decompress
        let bytes = response
            .bytes()
            .await
            .context(format!("Failed to read {} response body", operation))?;

        let decompressed = compression::read_transparent(&bytes)
            .context(format!("Failed to decompress {} response", operation))?;

        serde_json::from_slice(&decompressed)
            .context(format!("Failed to parse {} response", operation))
    }

    /// Generic helper: GET with Accept-Encoding: gzip, check status, parse response.
    async fn get_json<Resp>(&self, endpoint: &str, operation: &str) -> Result<Resp>
    where
        Resp: serde::de::DeserializeOwned,
    {
        let url = format!("{}{}", self.base_url, endpoint);
        let response = self
            .client
            .get(&url)
            .header("Accept-Encoding", "gzip")
            .send()
            .await
            .context(format!(
                "Failed to connect to remote server for {}",
                operation
            ))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("{} failed (HTTP {}): {}", operation, status, body);
        }

        let bytes = response
            .bytes()
            .await
            .context(format!("Failed to read {} response body", operation))?;

        let decompressed = compression::read_transparent(&bytes)
            .context(format!("Failed to decompress {} response", operation))?;

        serde_json::from_slice(&decompressed)
            .context(format!("Failed to parse {} response", operation))
    }

    /// Push changesets to the remote server.
    pub async fn push(&self, request: &PushRequest) -> Result<PushResponse> {
        self.post_json::<_, PushResponse>("/api/v1/push", request, "Push")
            .await
            .and_then(|resp| {
                resp.success
                    .then_some(resp.clone())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Push rejected: {}",
                            resp.error.unwrap_or_else(|| "Unknown error".into())
                        )
                    })
            })
    }

    /// Pull changesets from the remote server.
    pub async fn pull(&self, request: &PullRequest) -> Result<PullResponse> {
        self.post_json("/api/v1/pull", request, "Pull").await
    }

    /// Clone a repository from the remote server.
    pub async fn clone_repo(&self, request: &CloneRequest) -> Result<CloneResponse> {
        self.post_json("/api/v1/clone", request, "Clone").await
    }

    /// List all channels on the remote server.
    pub async fn list_channels(&self) -> Result<ListChannelsResponse> {
        self.get_json("/api/v1/channels", "List channels").await
    }

    /// Create a new channel on the remote server.
    pub async fn create_channel(
        &self,
        request: &CreateChannelRequest,
    ) -> Result<CreateChannelResponse> {
        self.post_json("/api/v1/channels", request, "Create channel")
            .await
    }

    /// Promote changesets from one channel to another on the remote server.
    pub async fn promote(&self, request: &PromoteRequest) -> Result<PromoteResponse> {
        self.post_json::<_, PromoteResponse>("/api/v1/promote", request, "Promote")
            .await
            .and_then(|resp| {
                resp.success
                    .then_some(resp.clone())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Promote rejected: {}",
                            resp.error.unwrap_or_else(|| "Unknown error".into())
                        )
                    })
            })
    }

    /// Check the health of the remote server.
    pub async fn health(&self) -> Result<HealthResponse> {
        self.get_json("/api/v1/health", "Health check").await
    }
}
