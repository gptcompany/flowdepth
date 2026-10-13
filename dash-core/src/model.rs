//! Shared JSON contract v1 (see `fixtures/sheets.json`). Changing a field here changes the
//! contract consumed by the sheet worker: coordinate before touching it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Every sheet uses the same columns so a renderer needs no per-sheet schema.
pub const COLUMNS: [&str; 5] = ["component", "status", "value", "source", "age_s"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Warn,
    Crit,
    Stale,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub value: String,
    pub severity: Severity,
    pub source: String,
    pub ts: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub id: String,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: String,
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetsResponse {
    pub generated_at: DateTime<Utc>,
    pub sheets: Vec<Sheet>,
}

impl Sheet {
    pub fn new(id: &str, title: &str, mut rows: Vec<Row>) -> Self {
        // Stable order: consumers diff successive responses.
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        Self {
            id: id.to_owned(),
            title: title.to_owned(),
            columns: COLUMNS.iter().map(|c| (*c).to_owned()).collect(),
            rows,
        }
    }
}

/// One logical reading, expanded into one cell per column sharing severity, source and ts.
pub struct Reading {
    pub component: String,
    pub status: String,
    pub value: String,
    pub severity: Severity,
    pub source: String,
    pub ts: Option<DateTime<Utc>>,
    pub age_s: Option<i64>,
}

impl Reading {
    pub fn into_row(self, id: impl Into<String>) -> Row {
        let age = self.age_s.map_or_else(|| "-".to_owned(), |a| a.to_string());
        let cell = |value: String| Cell {
            value,
            severity: self.severity,
            source: self.source.clone(),
            ts: self.ts,
        };
        let cells = vec![
            cell(self.component.clone()),
            cell(self.status.clone()),
            cell(self.value.clone()),
            cell(self.source.clone()),
            cell(age),
        ];
        Row {
            id: id.into(),
            cells,
        }
    }

    /// Source down or datum absent: the reason travels in the `value` column.
    pub fn missing(component: &str, source: &str, reason: &str) -> Self {
        Self {
            component: component.to_owned(),
            status: "missing".to_owned(),
            value: reason.to_owned(),
            severity: Severity::Missing,
            source: source.to_owned(),
            ts: None,
            age_s: None,
        }
    }
}

/// Whole seconds between `ts` and `now`, never negative (clock skew).
pub fn age_secs(ts: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    (now - ts).num_seconds().max(0)
}

/// `Stale` wins over the base severity once the datum is older than the source threshold.
pub fn with_staleness(base: Severity, age_s: i64, threshold_s: i64) -> Severity {
    if age_s > threshold_s {
        Severity::Stale
    } else {
        base
    }
}
