//! HTTP sync client for the WASM environment.
//!
//! Uses the browser's `fetch()` API via `web-sys` and `wasm-bindgen-futures`
//! instead of `reqwest`. The public API mirrors `dyna-cli/src/sync_client.rs`
//! so command logic can be shared.

use anyhow::{Context, Result};
use dyna_core::protocol::*;
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

    /// Generic POST JSON helper using the Fetch API.
    async fn post_json<Req, Resp>(&self, endpoint: &str, request: &Req, operation: &str) -> Result<Resp>
    where
        Req: serde::Serialize,
        Resp: serde::de::DeserializeOwned,
    {
        let url = format!("{}{}", self.base_url, endpoint);
        let body = serde_json::to_string(request)
            .context(format!("Failed to serialize {} request", operation))?;

        let headers = Headers::new()
            .map_err(|e| anyhow::anyhow!("Failed to create headers: {:?}", e))?;
        headers
            .set("Content-Type", "application/json")
            .map_err(|e| anyhow::anyhow!("Failed to set Content-Type: {:?}", e))?;

        let opts = RequestInit::new();
        opts.set_method("POST");
        opts.set_headers(&headers);
        opts.set_body(&JsValue::from_str(&body));
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

        let json_promise = resp
            .text()
            .map_err(|e| anyhow::anyhow!("{}: failed to get response text: {:?}", operation, e))?;
        let text_value = JsFuture::from(json_promise)
            .await
            .map_err(|e| anyhow::anyhow!("{}: failed to read response body: {:?}", operation, e))?;
        let text = text_value
            .as_string()
            .ok_or_else(|| anyhow::anyhow!("{}: response body is not a string", operation))?;

        serde_json::from_str(&text)
            .context(format!("Failed to parse {} response", operation))
    }

    /// Generic GET JSON helper using the Fetch API.
    async fn get_json<Resp>(&self, endpoint: &str, operation: &str) -> Result<Resp>
    where
        Resp: serde::de::DeserializeOwned,
    {
        let url = format!("{}{}", self.base_url, endpoint);

        let opts = RequestInit::new();
        opts.set_method("GET");
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

        let json_promise = resp
            .text()
            .map_err(|e| anyhow::anyhow!("{}: failed to get response text: {:?}", operation, e))?;
        let text_value = JsFuture::from(json_promise)
            .await
            .map_err(|e| anyhow::anyhow!("{}: failed to read response body: {:?}", operation, e))?;
        let text = text_value
            .as_string()
            .ok_or_else(|| anyhow::anyhow!("{}: response body is not a string", operation))?;

        serde_json::from_str(&text)
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

    pub async fn create_channel(&self, request: &CreateChannelRequest) -> Result<CreateChannelResponse> {
        self.post_json("/api/v1/channels", request, "Create channel").await
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
