#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use dash_core::config::Config;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Instant the recorded responses were crafted against (epoch 1791892800).
pub fn now() -> DateTime<Utc> {
    "2026-10-13T12:00:00Z".parse().unwrap()
}

pub fn fixture(rel: &str) -> String {
    let p: PathBuf = [env!("CARGO_MANIFEST_DIR"), "fixtures", rel]
        .iter()
        .collect();
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// The v1 contract example is a single file at the workspace root: the frontend embeds the same
/// bytes (`data::sheet`), so dash-core must not keep a copy of its own.
pub fn contract_fixture() -> String {
    let p: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "fixtures", "sheets.json"]
        .iter()
        .collect();
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

pub fn config(prom: Option<&str>, rekt: Option<&str>) -> Config {
    let env: HashMap<&str, String> = [("DASH_PROM_URL", prom), ("DASH_REKTSLUG_URL", rekt)]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k, v.to_owned())))
        .collect();
    Config::from_lookup(|k| env.get(k).cloned()).unwrap()
}

fn json(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body, "application/json")
}

pub async fn mount_prom(server: &MockServer, up: String, hl: String) {
    for (q, body) in [("up", up), (r#"{__name__=~"hl_.*"}"#, hl)] {
        Mock::given(method("GET"))
            .and(path("/api/v1/query"))
            .and(query_param("query", q))
            .respond_with(json(body))
            .mount(server)
            .await;
    }
}

pub async fn mount_alerts(server: &MockServer, body: String) {
    Mock::given(method("GET"))
        .and(path("/api/v1/alerts"))
        .respond_with(json(body))
        .mount(server)
        .await;
}

pub async fn mount_rektslug(server: &MockServer, health: String, summary: String) {
    for (p, body) in [("/health", health), ("/ops/summary", summary)] {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(json(body))
            .mount(server)
            .await;
    }
}

/// Every request answered with `status`.
pub async fn mount_all_status(server: &MockServer, status: u16) {
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(status))
        .mount(server)
        .await;
}

pub async fn mount_recorded(prom: &MockServer, rekt: &MockServer) {
    mount_prom(
        prom,
        fixture("recorded/prom_up.json"),
        fixture("recorded/prom_hl.json"),
    )
    .await;
    mount_alerts(prom, fixture("recorded/prometheus_alerts.json")).await;
    mount_rektslug(
        rekt,
        fixture("recorded/rektslug_health.json"),
        fixture("recorded/rektslug_summary.json"),
    )
    .await;
}
