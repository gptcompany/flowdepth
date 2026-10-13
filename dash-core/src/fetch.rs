//! Single read-only HTTP GET path. Every fetch is logged with source, duration, outcome and error.

use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use url::Url;

use crate::config::redact;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Fetcher {
    client: reqwest::Client,
}

impl Fetcher {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("dash-core/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { client })
    }

    /// GET `url` and decode the JSON body. The `Err` text is the cell-ready reason.
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        source: &'static str,
        url: Url,
    ) -> Result<T, String> {
        let endpoint = redact(&url).to_string();
        let started = Instant::now();
        let result = self.request(url).await;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match &result {
            Ok(_) => tracing::info!(source, endpoint, duration_ms, outcome = "ok", "fetch"),
            Err(error) => {
                tracing::warn!(
                    source,
                    endpoint,
                    duration_ms,
                    outcome = "error",
                    error,
                    "fetch"
                );
            }
        }
        result
    }

    async fn request<T: DeserializeOwned>(&self, url: Url) -> Result<T, String> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("request failed: {}", error_chain(e.without_url())))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("http {}", status.as_u16()));
        }
        let body = resp
            .bytes()
            .await
            .map_err(|e| format!("read body failed: {}", error_chain(e.without_url())))?;
        serde_json::from_slice(&body).map_err(|e| format!("invalid response: {e}"))
    }
}

/// reqwest's own Display hides the cause ("error sending request"); append the source chain
/// so the cell says e.g. "connection refused".
fn error_chain(e: reqwest::Error) -> String {
    let mut text = e.to_string();
    let mut cause = std::error::Error::source(&e);
    while let Some(c) = cause {
        text.push_str(": ");
        text.push_str(&c.to_string());
        cause = c.source();
    }
    text
}
