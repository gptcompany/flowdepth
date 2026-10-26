//! dash-core: `GET /v1/sheets` over Prometheus and rektslug, and the operator page at `GET /`.
//!
//! Read-only against every source. A failing source degrades to `missing` cells, never to a 500.

pub mod config;
pub mod fetch;
pub mod model;
pub mod sources;

use std::sync::Arc;

use axum::{Json, Router, extract::State, response::Html, routing::get};
use chrono::{DateTime, SubsecRound, Utc};

use crate::config::Config;
use crate::fetch::Fetcher;
use crate::model::SheetsResponse;

pub struct Collector {
    config: Config,
    fetcher: Fetcher,
}

impl Collector {
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        Ok(Self {
            config,
            fetcher: Fetcher::new()?,
        })
    }

    /// Queries all sources concurrently; `now` is injected so tests are deterministic.
    pub async fn collect(&self, now: DateTime<Utc>) -> SheetsResponse {
        let now = now.trunc_subsecs(0);
        let started = std::time::Instant::now();
        let (infra, alerts, rektslug, completeness, shadow) = tokio::join!(
            sources::prom::sheet(&self.fetcher, &self.config.prom, now),
            sources::alerts::sheet(&self.fetcher, &self.config.prom, now),
            sources::rektslug::sheet(&self.fetcher, &self.config.rektslug, now),
            sources::completeness::sheet(&self.fetcher, &self.config.prom, now),
            sources::shadow::sheet(&self.fetcher, &self.config.prom, now),
        );
        // New sheets go last so the positions of the existing ones do not move.
        let sheets = vec![infra, alerts, rektslug, completeness, shadow];
        tracing::info!(
            duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            rows = sheets.iter().map(|s| s.rows.len()).sum::<usize>(),
            "collect"
        );
        SheetsResponse {
            generated_at: now,
            sheets,
        }
    }
}

pub fn router(collector: Arc<Collector>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/v1/sheets", get(sheets))
        .with_state(collector)
}

/// Operator page: one static file, no external assets, polls `/v1/sheets` every 30 s.
pub const INDEX_HTML: &str = include_str!("../static/index.html");

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn sheets(State(collector): State<Arc<Collector>>) -> Json<SheetsResponse> {
    Json(collector.collect(Utc::now()).await)
}
