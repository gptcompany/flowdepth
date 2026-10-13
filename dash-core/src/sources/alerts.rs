//! Prometheus `GET /api/v1/alerts` → sheet "alerts": one row per pending or firing alert.
//!
//! Schema: <https://prometheus.io/docs/prometheus/latest/querying/api/#alerts>. The endpoint
//! carries no per-alert update time, so `activeAt` is the cell timestamp and no staleness rule
//! applies: an alert that has been firing for hours is not stale, it is firing.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{endpoint_url, unavailable_sheet};
use crate::config::Endpoint;
use crate::fetch::Fetcher;
use crate::model::{Reading, Row, Severity, Sheet, age_secs};

const SOURCE: &str = "prometheus";

#[derive(Debug, Deserialize)]
pub struct AlertsReply {
    status: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    data: Option<AlertsData>,
}

#[derive(Debug, Deserialize)]
struct AlertsData {
    alerts: Vec<PromAlert>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromAlert {
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    /// `pending` | `firing`.
    state: String,
    active_at: DateTime<Utc>,
    /// Value of the alert expression at the last evaluation, a Go-formatted float string.
    #[serde(default)]
    value: Option<String>,
}

/// `severity` label of a firing alert → cell severity. An unknown or absent label is `warn`:
/// the alert is firing, so it must not read as healthy.
pub fn map_severity(label: Option<&str>) -> Severity {
    match label.map(str::to_ascii_lowercase).as_deref() {
        Some("critical" | "crit" | "page" | "error" | "fatal") => Severity::Crit,
        Some("info" | "none" | "ok") => Severity::Ok,
        _ => Severity::Warn,
    }
}

/// One row per alert. A `pending` alert has not met its `for:` duration yet, so it is shown as
/// status `pending` with severity `warn` whatever its `severity` label says; escalation to
/// `crit` happens only once Prometheus reports it `firing`.
pub fn rows(reply: AlertsReply, source: &str, now: DateTime<Utc>) -> Result<Vec<Row>, String> {
    if reply.status != "success" {
        return Err(reply
            .error
            .unwrap_or_else(|| format!("status {}", reply.status)));
    }
    let alerts = reply
        .data
        .ok_or_else(|| "success reply without data".to_owned())?
        .alerts;
    Ok(alerts
        .into_iter()
        .map(|a| {
            let name = a.labels.get("alertname").map_or("?", String::as_str);
            let component = match a.labels.get("instance") {
                Some(i) => format!("{name} ({i})"),
                None => name.to_owned(),
            };
            let sev_label = a.labels.get("severity").map(String::as_str);
            let (status, severity) = match a.state.as_str() {
                "firing" => (
                    sev_label.unwrap_or("unknown").to_owned(),
                    map_severity(sev_label),
                ),
                other => (other.to_owned(), Severity::Warn),
            };
            let text = a
                .annotations
                .get("summary")
                .or_else(|| a.annotations.get("description"));
            // The expression value is the only datum an unannotated alert carries.
            let value = match (text, a.value.as_deref().filter(|v| !v.is_empty())) {
                (Some(t), Some(v)) => format!("{t} (value={v})"),
                (Some(t), None) => t.clone(),
                (None, Some(v)) => format!("value={v}"),
                (None, None) => name.to_owned(),
            };
            // The label set is the alert's identity in Prometheus.
            let id = a
                .labels
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",");
            Reading {
                component,
                status,
                value,
                severity,
                source: source.to_owned(),
                ts: Some(a.active_at),
                age_s: Some(age_secs(a.active_at, now)),
            }
            .into_row(format!("alert:{id}"))
        })
        .collect())
}

pub async fn sheet(fetcher: &Fetcher, ep: &Endpoint, now: DateTime<Utc>) -> Sheet {
    if let Some(sheet) = unavailable_sheet("alerts", "Alerts", "alerts:prometheus", SOURCE, ep) {
        return sheet;
    }
    let Endpoint::Ready(base) = ep else {
        unreachable!("unavailable_sheet covers non-ready endpoints")
    };
    let label = ep.label();
    let url = endpoint_url(base, "api/v1/alerts", &[]);
    let rows = match fetcher.get_json::<AlertsReply>(SOURCE, url).await {
        Ok(reply) => {
            rows(reply, &label, now).unwrap_or_else(|reason| vec![missing_row(&label, &reason)])
        }
        Err(reason) => vec![missing_row(&label, &reason)],
    };
    Sheet::new("alerts", "Alerts", rows)
}

fn missing_row(label: &str, reason: &str) -> Row {
    Reading::missing(SOURCE, label, reason).into_row("alerts:prometheus")
}
