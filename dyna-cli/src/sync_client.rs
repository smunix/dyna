//! HTTP sync client for communicating with the remote Dyna server.
//!
//! This module encapsulates all network interactions between the CLI and the
//! remote server, providing a clean async API for changeset-based push, pull,
//! clone, promote, and channel management operations.
//!
//! All operations exchange [`Changeset`] objects (not individual patches),
//! consistent with the Jujutsu-inspired changeset-centric model.

use anyhow::{Context, Result, bail};
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

    /// Push patches to the remote server.
    pub async fn push(&self, request: &PushRequest) -> Result<PushResponse> {
        let url = format!("{}/api/v1/push", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await
            .context("Failed to connect to remote server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Push failed (HTTP {}): {}", status, body);
        }

        let push_response: PushResponse = response
            .json()
            .await
            .context("Failed to parse push response")?;

        if !push_response.success {
            bail!(
                "Push rejected: {}",
                push_response.error.unwrap_or_else(|| "Unknown error".into())
            );
        }

        Ok(push_response)
    }

    /// Pull patches from the remote server.
    pub async fn pull(&self, request: &PullRequest) -> Result<PullResponse> {
        let url = format!("{}/api/v1/pull", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await
            .context("Failed to connect to remote server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Pull failed (HTTP {}): {}", status, body);
        }

        let pull_response: PullResponse = response
            .json()
            .await
            .context("Failed to parse pull response")?;

        Ok(pull_response)
    }

    /// Clone a repository from the remote server.
    pub async fn clone_repo(&self, request: &CloneRequest) -> Result<CloneResponse> {
        let url = format!("{}/api/v1/clone", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await
            .context("Failed to connect to remote server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Clone failed (HTTP {}): {}", status, body);
        }

        let clone_response: CloneResponse = response
            .json()
            .await
            .context("Failed to parse clone response")?;

        Ok(clone_response)
    }

    /// List all channels on the remote server.
    pub async fn list_channels(&self) -> Result<ListChannelsResponse> {
        let url = format!("{}/api/v1/channels", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to remote server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("List channels failed (HTTP {}): {}", status, body);
        }

        let list_response: ListChannelsResponse = response
            .json()
            .await
            .context("Failed to parse channels response")?;

        Ok(list_response)
    }

    /// Create a new channel on the remote server.
    pub async fn create_channel(&self, request: &CreateChannelRequest) -> Result<CreateChannelResponse> {
        let url = format!("{}/api/v1/channels", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await
            .context("Failed to connect to remote server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Create channel failed (HTTP {}): {}", status, body);
        }

        let create_response: CreateChannelResponse = response
            .json()
            .await
            .context("Failed to parse create channel response")?;

        Ok(create_response)
    }

    /// Promote patches from one channel to another on the remote server.
    pub async fn promote(&self, request: &PromoteRequest) -> Result<PromoteResponse> {
        let url = format!("{}/api/v1/promote", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await
            .context("Failed to connect to remote server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Promote failed (HTTP {}): {}", status, body);
        }

        let promote_response: PromoteResponse = response
            .json()
            .await
            .context("Failed to parse promote response")?;

        if !promote_response.success {
            bail!(
                "Promote rejected: {}",
                promote_response.error.unwrap_or_else(|| "Unknown error".into())
            );
        }

        Ok(promote_response)
    }

    /// Check the health of the remote server.
    pub async fn health(&self) -> Result<HealthResponse> {
        let url = format!("{}/api/v1/health", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to remote server")?;

        let health: HealthResponse = response
            .json()
            .await
            .context("Failed to parse health response")?;

        Ok(health)
    }
}
