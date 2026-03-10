//! HTTP sync client for the WASM environment.
//!
//! Uses the browser's `fetch()` API via `web-sys` and `wasm-bindgen-futures`
//! instead of `reqwest`. The public API mirrors `dyna-cli/src/sync_client.rs`
//! so command logic can be shared.
//!
//! All request bodies are gzip-compressed and sent with `Content-Encoding: gzip`.
//! The client also sends `Accept-Encoding: gzip` to request compressed responses.
//! Response bodies are transparently decompressed using `dyna_core::compression`.

use anyhow::{Context, Result};
use dyna_core::compression;
use dyna_core::protocol::*;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, RequestMode, Response};

/// Client for interacting with the Dyna remote server from the browser.
pub struct SyncClient {
    base_url: String,
}

impl SyncClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Get the global `Window` object.
    fn window() -> Result<web_sys::Window> {
        web_sys::window().ok_or_else(|| anyhow::anyhow!("No global window object"))
    }

    /// Read response body as bytes, transparently decompressing gzip if needed.
    async fn read_response_bytes(resp: &Response, operation: &str) -> Result<Vec<u8>> {
        let array_buffer_promise = resp
            .array_buffer()
            .map_err(|e| anyhow::anyhow!("{}: failed to get response body: {:?}", operation, e))?;
        let array_buffer = JsFuture::from(array_buffer_promise)
            .await
            .map_err(|e| anyhow::anyhow!("{}: failed to read response body: {:?}", operation, e))?;
        let uint8_array = Uint8Array::new(&array_buffer);
        let bytes = uint8_array.to_vec();
        compression::read_transparent(&bytes)
            .map_err(|e| anyhow::anyhow!("{}: decompression failed: {}", operation, e))
    }

    /// Generic POST JSON helper using the Fetch API with gzip compression.
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

        // Serialize to JSON and gzip-compress
        let compressed_body = compression::compress_json(request)
            .map_err(|e| anyhow::anyhow!("Failed to compress {} request: {}", operation, e))?;

        let headers = Headers::new()
            .map_err(|e| anyhow::anyhow!("Failed to create headers: {:?}", e))?;
        headers
            .set("Content-Type", "application/json")
            .map_err(|e| anyhow::anyhow!("Failed to set Content-Type: {:?}", e))?;
        headers
            .set("Content-Encoding", "gzip")
            .map_err(|e| anyhow::anyhow!("Failed to set Content-Encoding: {:?}", e))?;
        headers
            .set("Accept-Encoding", "gzip")
            .map_err(|e| anyhow::anyhow!("Failed to set Accept-Encoding: {:?}", e))?;

        // Convert compressed bytes to Uint8Array for fetch body
        let body_array = Uint8Array::from(compressed_body.as_slice());

        let opts = RequestInit::new();
        opts.set_method("POST");
        opts.set_headers(&headers);
        opts.set_body(&body_array.into());
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| anyhow::anyhow!("Failed to create request: {:?}", e))?;

        let window = Self::window()?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| anyhow::anyhow!("{} fetch failed: {:?}", operation, e))?;

        let resp: Response = resp_value
            .dyn_into()
            .map_err(|_| anyhow::anyhow!("{}: response is not a Response object", operation))?;

        if !resp.ok() {
            let status = resp.status();
            return Err(anyhow::anyhow!("{} failed with status {}", operation, status));
        }

        let bytes = Self::read_response_bytes(&resp, operation).await?;
        serde_json::from_slice(&bytes)
            .context(format!("Failed to parse {} response", operation))
    }

    /// Generic GET JSON helper using the Fetch API with Accept-Encoding: gzip.
    async fn get_json<Resp>(&self, endpoint: &str, operation: &str) -> Result<Resp>
    where
        Resp: serde::de::DeserializeOwned,
    {
        let url = format!("{}{}", self.base_url, endpoint);

        let headers = Headers::new()
            .map_err(|e| anyhow::anyhow!("Failed to create headers: {:?}", e))?;
        headers
            .set("Accept-Encoding", "gzip")
            .map_err(|e| anyhow::anyhow!("Failed to set Accept-Encoding: {:?}", e))?;

        let opts = RequestInit::new();
        opts.set_method("GET");
        opts.set_headers(&headers);
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| anyhow::anyhow!("Failed to create request: {:?}", e))?;

        let window = Self::window()?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| anyhow::anyhow!("{} fetch failed: {:?}", operation, e))?;

        let resp: Response = resp_value
            .dyn_into()
            .map_err(|_| anyhow::anyhow!("{}: response is not a Response object", operation))?;

        if !resp.ok() {
            let status = resp.status();
            return Err(anyhow::anyhow!("{} failed with status {}", operation, status));
        }

        let bytes = Self::read_response_bytes(&resp, operation).await?;
        serde_json::from_slice(&bytes)
            .context(format!("Failed to parse {} response", operation))
    }

    // -----------------------------------------------------------------------
    // Public API (mirrors dyna-cli sync_client)
    // -----------------------------------------------------------------------

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

    pub async fn pull(&self, request: &PullRequest) -> Result<PullResponse> {
        self.post_json("/api/v1/pull", request, "Pull").await
    }

    pub async fn clone_repo(&self, request: &CloneRequest) -> Result<CloneResponse> {
        self.post_json("/api/v1/clone", request, "Clone").await
    }

    pub async fn list_channels(&self) -> Result<ListChannelsResponse> {
        self.get_json("/api/v1/channels", "List channels").await
    }

    pub async fn create_channel(
        &self,
        request: &CreateChannelRequest,
    ) -> Result<CreateChannelResponse> {
        self.post_json("/api/v1/channels", request, "Create channel")
            .await
    }

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

    pub async fn health(&self) -> Result<HealthResponse> {
        self.get_json("/api/v1/health", "Health check").await
    }
}
