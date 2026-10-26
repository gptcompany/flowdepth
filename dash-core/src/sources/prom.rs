//! Prometheus HTTP API → sheet "infra": `up` per job/instance plus `hl_*` series.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{endpoint_url, unavailable_sheet};
use crate::config::{Endpoint, PROM_STALE_S};
use crate::fetch::Fetcher;
use crate::model::{Reading, Row, Severity, Sheet, age_secs, with_staleness};

const SOURCE: &str = "prometheus";
const HL_SELECTOR: &str = r#"{__name__=~"hl_.*"}"#;

#[derive(Debug, Deserialize)]
pub struct PromReply {
    status: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    data: Option<PromData>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PromData {
    result: Vec<PromSample>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PromSample {
    pub(crate) metric: BTreeMap<String, String>,
    /// `[unix_seconds, "value"]`
    pub(crate) value: (f64, String),
}

pub(crate) fn samples(reply: PromReply) -> Result<Vec<PromSample>, String> {
    if reply.status != "success" {
        return Err(reply
            .error
            .unwrap_or_else(|| format!("status {}", reply.status)));
    }
    reply
        .data
        .map(|d| d.result)
        .ok_or_else(|| "success reply without data".to_owned())
}

fn sample_ts(sample: &PromSample) -> Option<DateTime<Utc>> {
    // Seconds precision keeps the contract output stable.
    DateTime::from_timestamp(sample.value.0.floor() as i64, 0)
}

pub(crate) fn label<'a>(sample: &'a PromSample, key: &str) -> &'a str {
    sample.metric.get(key).map_or("?", String::as_str)
}

/// One row per `up` series.
pub fn up_rows(reply: PromReply, source: &str, now: DateTime<Utc>) -> Result<Vec<Row>, String> {
    Ok(samples(reply)?
        .iter()
        .map(|s| {
            let (job, instance) = (label(s, "job"), label(s, "instance"));
            let (status, base) = match s.value.1.as_str() {
                "1" => ("up", Severity::Ok),
                "0" => ("down", Severity::Crit),
                _ => ("unknown", Severity::Warn),
            };
            let ts = sample_ts(s);
            let age = ts.map(|t| age_secs(t, now));
            Reading {
                component: format!("{job}/{instance}"),
                status: status.to_owned(),
                value: s.value.1.clone(),
                severity: age.map_or(base, |a| with_staleness(base, a, PROM_STALE_S)),
                source: source.to_owned(),
                ts,
                age_s: age,
            }
            .into_row(format!("up:{job}:{instance}"))
        })
        .collect())
}

/// One row per `hl_*` series, labels folded into the component name.
pub fn hl_rows(reply: PromReply, source: &str, now: DateTime<Utc>) -> Result<Vec<Row>, String> {
    Ok(samples(reply)?
        .iter()
        .map(|s| {
            let name = label(s, "__name__");
            let labels = s
                .metric
                .iter()
                .filter(|(k, _)| k.as_str() != "__name__")
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",");
            let component = format!("{name}{{{labels}}}");
            let finite = s.value.1.parse::<f64>().is_ok_and(f64::is_finite);
            let (status, base) = if finite {
                ("present", Severity::Ok)
            } else {
                ("non-finite", Severity::Warn)
            };
            let ts = sample_ts(s);
            let age = ts.map(|t| age_secs(t, now));
            Reading {
                component: component.clone(),
                status: status.to_owned(),
                value: s.value.1.clone(),
                severity: age.map_or(base, |a| with_staleness(base, a, PROM_STALE_S)),
                source: source.to_owned(),
                ts,
                age_s: age,
            }
            .into_row(format!("hl:{component}"))
        })
        .collect())
}

pub async fn sheet(fetcher: &Fetcher, ep: &Endpoint, now: DateTime<Utc>) -> Sheet {
    if let Some(sheet) = unavailable_sheet("infra", "Infra", "infra:prometheus", SOURCE, ep) {
        return sheet;
    }
    let Endpoint::Ready(base) = ep else {
        unreachable!("unavailable_sheet covers non-ready endpoints")
    };
    let label = ep.label();
    let query = |q: &'static str| {
        fetcher.get_json::<PromReply>(SOURCE, endpoint_url(base, "api/v1/query", &[("query", q)]))
    };
    let (up, hl) = tokio::join!(query("up"), query(HL_SELECTOR));

    let mut rows = Vec::new();
    match up.and_then(|r| up_rows(r, &label, now)) {
        Ok(r) => rows.extend(r),
        Err(reason) => {
            rows.push(Reading::missing(SOURCE, &label, &reason).into_row("infra:prometheus"))
        }
    }
    match hl.and_then(|r| hl_rows(r, &label, now)) {
        Ok(r) => rows.extend(r),
        Err(reason) => rows.push(Reading::missing("hl_*", &label, &reason).into_row("infra:hl")),
    }
    Sheet::new("infra", "Infra", rows)
}
