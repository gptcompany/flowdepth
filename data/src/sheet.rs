//! Sheet pane: dash-core `GET /v1/sheets` contract (v1) and the pure view model
//! derived from it. Kept free of UI types so it can be unit tested without a renderer.

use serde::{Deserialize, Serialize};

pub const DEFAULT_URL: &str = "http://127.0.0.1:8790";
pub const URL_ENV: &str = "DASH_CORE_URL";

/// Per-pane persisted state: only the selected tab survives a restart.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub selected: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Warn,
    Crit,
    Stale,
    Missing,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Cell {
    pub value: String,
    pub severity: Severity,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub ts: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Row {
    pub id: String,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Sheet {
    pub id: String,
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Sheets {
    pub generated_at: String,
    pub sheets: Vec<Sheet>,
}

/// Base URL of dash-core: `DASH_CORE_URL` when set and non-empty, else the local default.
pub fn base_url() -> String {
    base_url_from(std::env::var(URL_ENV).ok().as_deref())
}

fn base_url_from(value: Option<&str>) -> String {
    value
        .map(|v| v.trim().trim_end_matches('/'))
        .filter(|v| !v.is_empty())
        .unwrap_or(DEFAULT_URL)
        .to_string()
}

pub fn endpoint() -> String {
    format!("{}/v1/sheets", base_url())
}

/// Text shown when dash-core cannot be reached or answers with something unusable.
pub fn unreachable_banner(error: &str) -> String {
    format!("dash-core non raggiungibile: {error}")
}

#[derive(Debug, Clone, PartialEq)]
pub struct CellView {
    pub text: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RowView {
    pub id: String,
    pub cells: Vec<CellView>,
    pub has_crit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableView {
    pub id: String,
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<RowView>,
}

impl TableView {
    pub fn crit_rows(&self) -> usize {
        self.rows.iter().filter(|r| r.has_crit).count()
    }
}

/// Table view of one sheet: rows with a `crit` cell first, source order kept otherwise.
/// Cells beyond the declared columns are dropped and missing ones are not invented:
/// a short row is shown short, never padded with a fake severity.
pub fn table_view(sheet: &Sheet) -> TableView {
    let width = sheet.columns.len();
    let mut rows: Vec<RowView> = sheet
        .rows
        .iter()
        .map(|row| RowView {
            id: row.id.clone(),
            has_crit: row.cells.iter().any(|c| c.severity == Severity::Crit),
            cells: row
                .cells
                .iter()
                .take(width)
                .map(|c| CellView {
                    text: c.value.clone(),
                    severity: c.severity,
                })
                .collect(),
        })
        .collect();
    // `sort_by_key` is stable, so rows of equal rank keep dash-core's order.
    rows.sort_by_key(|r| !r.has_crit);
    TableView {
        id: sheet.id.clone(),
        title: sheet.title.clone(),
        columns: sheet.columns.clone(),
        rows,
    }
}

pub fn tab_views(sheets: &Sheets) -> Vec<TableView> {
    sheets.sheets.iter().map(table_view).collect()
}

/// Selected tab index: the persisted id when still present, otherwise the first tab.
pub fn selected_index(tables: &[TableView], selected: Option<&str>) -> Option<usize> {
    if tables.is_empty() {
        return None;
    }
    Some(
        selected
            .and_then(|id| tables.iter().position(|t| t.id == id))
            .unwrap_or(0),
    )
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const FIXTURE: &str = include_str!("../../fixtures/sheets.json");

    pub fn fixture() -> Sheets {
        serde_json::from_str(FIXTURE).expect("fixture matches the v1 contract")
    }

    #[test]
    fn fixture_has_three_sheets_and_every_severity() {
        let sheets = fixture();
        let ids: Vec<_> = sheets.sheets.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["infra", "alerts", "rektslug"]);
        for sheet in &sheets.sheets {
            for row in &sheet.rows {
                assert_eq!(row.cells.len(), sheet.columns.len(), "{}", row.id);
            }
            for severity in [
                Severity::Ok,
                Severity::Warn,
                Severity::Crit,
                Severity::Stale,
                Severity::Missing,
            ] {
                assert!(
                    sheet
                        .rows
                        .iter()
                        .flat_map(|r| &r.cells)
                        .any(|c| c.severity == severity),
                    "{} lacks {severity:?}",
                    sheet.id
                );
            }
        }
    }

    #[test]
    fn crit_rows_come_first_and_order_is_stable() {
        let tables = tab_views(&fixture());
        let infra = &tables[0];
        assert_eq!(infra.title, "Infra");
        assert_eq!(infra.rows[0].id, "infra/questdb");
        assert!(infra.rows[0].has_crit);
        let rest: Vec<_> = infra.rows[1..].iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            rest,
            [
                "infra/postgres",
                "infra/redis",
                "infra/ingest",
                "infra/exporter"
            ]
        );
        assert_eq!(infra.crit_rows(), 1);
    }

    #[test]
    fn cell_severity_is_carried_to_the_view() {
        let tables = tab_views(&fixture());
        let questdb = &tables[0].rows[0];
        assert_eq!(questdb.cells[0].severity, Severity::Ok);
        assert_eq!(questdb.cells[1].text, "down");
        assert_eq!(questdb.cells[1].severity, Severity::Crit);
    }

    #[test]
    fn unknown_severity_is_rejected_not_defaulted() {
        let bad = r#"{"value":"x","severity":"purple"}"#;
        assert!(serde_json::from_str::<Cell>(bad).is_err());
    }

    #[test]
    fn selected_tab_falls_back_to_first() {
        let tables = tab_views(&fixture());
        assert_eq!(selected_index(&tables, Some("alerts")), Some(1));
        assert_eq!(selected_index(&tables, Some("gone")), Some(0));
        assert_eq!(selected_index(&tables, None), Some(0));
        assert_eq!(selected_index(&[], Some("alerts")), None);
    }

    #[test]
    fn base_url_defaults_and_trims() {
        assert_eq!(base_url_from(None), DEFAULT_URL);
        assert_eq!(base_url_from(Some("  ")), DEFAULT_URL);
        assert_eq!(base_url_from(Some("http://h:1/")), "http://h:1");
    }

    #[test]
    fn banner_names_dash_core() {
        assert_eq!(
            unreachable_banner("connection refused"),
            "dash-core non raggiungibile: connection refused"
        );
    }
}
