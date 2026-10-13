//! Alertmanager /api/v2/alerts → sheet "alerts": one row per active alert.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{endpoint_url, unavailable_sheet};
use crate::config::{ALERT_STALE_S, Endpoint};
use crate::fetch::Fetcher;
use crate::model::{Reading, Row, Severity, Sheet, age_secs, with_staleness};

const SOURCE: &str = "alertmanager";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmAlert {
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    starts_at: DateTime<Utc>,
    #[serde(default)]
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    fingerprint: Option<String>,
}

/// `severity` label → cell severity. An active alert with an unknown or absent label is `warn`:
/// it is firing, so it must not read as healthy.
pub fn map_severity(label: Option<&str>) -> Severity {
    match label.map(str::to_ascii_lowercase).as_deref() {
        Some("critical" | "crit" | "page" | "error" | "fatal") => Severity::Crit,
        Some("info" | "none" | "ok") => Severity::Ok,
        _ => Severity::Warn,
    }
}

pub fn rows(alerts: Vec<AmAlert>, source: &str, now: DateTime<Utc>) -> Vec<Row> {
    alerts
        .into_iter()
        .map(|a| {
            let name = a.labels.get("alertname").map_or("?", String::as_str);
            let component = match a.labels.get("instance") {
                Some(i) => format!("{name} ({i})"),
                None => name.to_owned(),
            };
            let sev_label = a.labels.get("severity").map(String::as_str);
            let ts = a.updated_at.unwrap_or(a.starts_at);
            let value = a
                .annotations
                .get("summary")
                .or_else(|| a.annotations.get("description"))
                .cloned()
                .unwrap_or_else(|| name.to_owned());
            let id = a.fingerprint.clone().unwrap_or_else(|| {
                // No fingerprint (older Alertmanager): the label set is the identity.
                a.labels
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",")
            });
            Reading {
                component,
                status: sev_label.unwrap_or("unknown").to_owned(),
                value,
                severity: with_staleness(map_severity(sev_label), age_secs(ts, now), ALERT_STALE_S),
                source: source.to_owned(),
                ts: Some(ts),
                age_s: Some(age_secs(a.starts_at, now)),
            }
            .into_row(format!("alert:{id}"))
        })
        .collect()
}

pub async fn sheet(fetcher: &Fetcher, ep: &Endpoint, now: DateTime<Utc>) -> Sheet {
    if let Some(sheet) = unavailable_sheet("alerts", "Alerts", "alerts:alertmanager", SOURCE, ep) {
        return sheet;
    }
    let Endpoint::Ready(base) = ep else {
        unreachable!("unavailable_sheet covers non-ready endpoints")
    };
    let label = ep.label();
    let url = endpoint_url(
        base,
        "api/v2/alerts",
        &[
            ("active", "true"),
            ("silenced", "false"),
            ("inhibited", "false"),
        ],
    );
    let rows = match fetcher.get_json::<Vec<AmAlert>>(SOURCE, url).await {
        Ok(alerts) => rows(alerts, &label, now),
        Err(reason) => {
            vec![Reading::missing(SOURCE, &label, &reason).into_row("alerts:alertmanager")]
        }
    };
    Sheet::new("alerts", "Alerts", rows)
}
