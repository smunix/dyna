//! HTTP sync client for the WASM environment.
//!
//! Uses the browser's `fetch()` API via `web-sys` and `wasm-bindgen-futures`.
//! Mirrors the subset of `dyna-cli/src/sync_client.rs` needed by lazy-wasm
//! (clone + pull only).

use anyhow::{Context, Result};
use dyna_core::compression;
use dyna_core::protocol::*;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, RequestMode, Response};

pub struct SyncClient {
    base_url: String,
}

impl SyncClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    fn window() -> Result<web_sys::Window> {
        web_sys::window().ok_or_else(|| anyhow::anyhow!("No global window object"))
    }

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

    pub async fn clone_repo(&self, request: &CloneRequest) -> Result<CloneResponse> {
        self.post_json("/api/v1/clone", request, "Clone").await
    }

    pub async fn pull(&self, request: &PullRequest) -> Result<PullResponse> {
        self.post_json("/api/v1/pull", request, "Pull").await
    }
}
