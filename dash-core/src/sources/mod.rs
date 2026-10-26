pub mod alerts;
pub mod completeness;
pub mod prom;
pub mod rektslug;
pub mod shadow;

use chrono::{DateTime, Utc};
use url::Url;

use crate::config::Endpoint;
use crate::model::{Reading, Severity, Sheet, age_secs};

/// Joins `path` onto the (trailing-slash normalised) base and appends query pairs.
pub(crate) fn endpoint_url(base: &Url, path: &str, params: &[(&str, &str)]) -> Url {
    // `path` is a compile-time literal, so the join cannot fail on a valid base.
    let mut url = base.join(path).unwrap_or_else(|_| base.clone());
    if !params.is_empty() {
        url.query_pairs_mut().extend_pairs(params);
    }
    url
}

/// Sheet for a source whose env var is unset or invalid: one missing row, never an error.
pub(crate) fn unavailable_sheet(
    id: &str,
    title: &str,
    row_id: &str,
    component: &str,
    ep: &Endpoint,
) -> Option<Sheet> {
    let reason = ep.unavailable_reason()?;
    let row = Reading::missing(component, &ep.label(), &reason).into_row(row_id);
    Some(Sheet::new(id, title, vec![row]))
}

/// Prometheus sample value as a finite number; `NaN`/`Inf`/garbage count as absent.
pub(crate) fn finite(raw: &str) -> Option<f64> {
    raw.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Age of a producer's last run (unix seconds) → the severity its values inherit, and its own
/// row. Older than `stale_s`: `stale`, «vecchio di N minuti». Absent: the row is «assente» and the
/// values `warn`, so a value of unknown age never reads as `ok`.
pub(crate) fn run_freshness(
    component: String,
    run: Option<f64>,
    now: DateTime<Utc>,
    stale_s: i64,
) -> (Severity, Reading) {
    let Some(ts) = run.and_then(|v| DateTime::from_timestamp(v as i64, 0)) else {
        return (Severity::Warn, Reading::missing(&component, "", "assente"));
    };
    let age = age_secs(ts, now);
    let (severity, status) = if age > stale_s {
        (Severity::Stale, format!("vecchio di {} minuti", age / 60))
    } else {
        (Severity::Ok, "aggiornato".to_owned())
    };
    let row = Reading {
        component,
        status,
        value: format!("{} min", age / 60),
        severity,
        source: String::new(),
        ts: Some(ts),
        age_s: Some(age),
    };
    (severity, row)
}

/// A value produced by the run in `run`: it carries the run's timestamp and age.
pub(crate) fn derived(run: &Reading, base: Severity, component: String, value: String) -> Reading {
    let status = if base == Severity::Warn {
        "età della fonte ignota"
    } else {
        "presente"
    };
    Reading {
        component,
        status: status.to_owned(),
        value,
        severity: base,
        source: String::new(),
        ts: run.ts,
        age_s: run.age_s,
    }
}
