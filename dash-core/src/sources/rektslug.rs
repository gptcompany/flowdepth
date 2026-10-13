//! rektslug FastAPI (port 8002) → sheet "rektslug".
//!
//! Endpoints (gptcompany/rektslug, `src/liquidationheatmap/api`):
//! - `GET /health` → `{"status":"ok","service","version",...}` (liveness; used by its compose healthcheck)
//! - `GET /ops/summary` → ops envelope; `details.data_freshness["<table>:<symbol>"]` carries
//!   `data_as_of`, `sla_secs`, `stale`, `reason` per QuestDB stream.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{endpoint_url, unavailable_sheet};
use crate::config::{Endpoint, REKTSLUG_STALE_S};
use crate::fetch::Fetcher;
use crate::model::{Reading, Row, Severity, Sheet, age_secs, with_staleness};

const SOURCE: &str = "rektslug";

#[derive(Debug, Deserialize)]
pub struct Health {
    status: String,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Summary {
    status: String,
    #[serde(default)]
    last_error: Option<String>,
    #[serde(default)]
    details: SummaryDetails,
}

#[derive(Debug, Default, Deserialize)]
struct SummaryDetails {
    #[serde(default)]
    data_freshness: Option<BTreeMap<String, Stream>>,
}

#[derive(Debug, Deserialize)]
struct Stream {
    #[serde(default)]
    data_as_of: Option<DateTime<Utc>>,
    #[serde(default)]
    sla_secs: Option<i64>,
    #[serde(default)]
    stale: bool,
    #[serde(default)]
    reason: Option<String>,
}

pub fn health_row(h: &Health, source: &str, now: DateTime<Utc>) -> Row {
    let severity = if h.status.eq_ignore_ascii_case("ok") {
        Severity::Ok
    } else {
        Severity::Crit
    };
    Reading {
        component: "rektslug /health".to_owned(),
        status: h.status.clone(),
        value: h.version.clone().unwrap_or_else(|| "-".to_owned()),
        severity,
        source: source.to_owned(),
        ts: Some(now),
        age_s: Some(0),
    }
    .into_row("rektslug:health")
}

/// Envelope status plus one row per `<table>:<symbol>` stream with its last data timestamp.
pub fn summary_rows(s: &Summary, source: &str, now: DateTime<Utc>) -> Vec<Row> {
    let envelope = match s.status.as_str() {
        "HEALTHY" => Reading {
            component: "rektslug /ops/summary".to_owned(),
            status: "HEALTHY".to_owned(),
            value: "-".to_owned(),
            severity: Severity::Ok,
            source: source.to_owned(),
            ts: Some(now),
            age_s: Some(0),
        },
        "DEGRADED" | "BLOCKED" => Reading {
            component: "rektslug /ops/summary".to_owned(),
            status: s.status.clone(),
            value: s.last_error.clone().unwrap_or_else(|| "-".to_owned()),
            severity: if s.status == "BLOCKED" {
                Severity::Crit
            } else {
                Severity::Warn
            },
            source: source.to_owned(),
            ts: Some(now),
            age_s: Some(0),
        },
        other => Reading::missing(
            "rektslug /ops/summary",
            source,
            &s.last_error
                .clone()
                .unwrap_or_else(|| format!("reported {other}")),
        ),
    };
    let mut rows = vec![envelope.into_row("rektslug:ops")];

    if let Some(streams) = &s.details.data_freshness {
        for (key, st) in streams {
            let row = match st.data_as_of {
                Some(ts) => {
                    let age = age_secs(ts, now);
                    let threshold = st.sla_secs.unwrap_or(REKTSLUG_STALE_S);
                    let base = if st.stale {
                        Severity::Stale
                    } else {
                        Severity::Ok
                    };
                    Reading {
                        component: key.clone(),
                        status: if st.stale { "stale" } else { "fresh" }.to_owned(),
                        value: ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        severity: with_staleness(base, age, threshold),
                        source: source.to_owned(),
                        ts: Some(ts),
                        age_s: Some(age),
                    }
                }
                None => Reading::missing(
                    key,
                    source,
                    st.reason.as_deref().unwrap_or("no data_as_of reported"),
                ),
            };
            rows.push(row.into_row(format!("rektslug:{key}")));
        }
    }
    rows
}

pub async fn sheet(fetcher: &Fetcher, ep: &Endpoint, now: DateTime<Utc>) -> Sheet {
    if let Some(sheet) = unavailable_sheet("rektslug", "Rektslug", "rektslug:health", SOURCE, ep) {
        return sheet;
    }
    let Endpoint::Ready(base) = ep else {
        unreachable!("unavailable_sheet covers non-ready endpoints")
    };
    let label = ep.label();
    let (health, summary) = tokio::join!(
        fetcher.get_json::<Health>(SOURCE, endpoint_url(base, "health", &[])),
        fetcher.get_json::<Summary>(SOURCE, endpoint_url(base, "ops/summary", &[])),
    );

    let mut rows = Vec::new();
    match health {
        Ok(h) => rows.push(health_row(&h, &label, now)),
        Err(reason) => rows.push(
            Reading::missing("rektslug /health", &label, &reason).into_row("rektslug:health"),
        ),
    }
    match summary {
        Ok(s) => rows.extend(summary_rows(&s, &label, now)),
        Err(reason) => {
            rows.push(
                Reading::missing("rektslug /ops/summary", &label, &reason).into_row("rektslug:ops"),
            );
        }
    }
    Sheet::new("rektslug", "Rektslug", rows)
}
