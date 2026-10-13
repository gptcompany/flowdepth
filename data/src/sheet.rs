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

impl Severity {
    /// Display rank, lower first: `crit > warn > stale > missing > ok`.
    fn rank(self) -> u8 {
        match self {
            Severity::Crit => 0,
            Severity::Warn => 1,
            Severity::Stale => 2,
            Severity::Missing => 3,
            Severity::Ok => 4,
        }
    }
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

/// `url` without userinfo, query and fragment, safe to log or show. An unparsable
/// value is never echoed back: it may still carry credentials.
pub fn redact_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => "<invalid url>".to_string(),
    }
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
    /// Most severe cell of the row (`Ok` for a row without cells): the sort key.
    pub worst: Severity,
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

/// Table view of one sheet: rows ordered by their worst cell severity
/// (`crit > warn > stale > missing > ok`), source order kept within the same severity.
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
            worst: row
                .cells
                .iter()
                .map(|c| c.severity)
                .min_by_key(|s| s.rank())
                .unwrap_or(Severity::Ok),
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
    rows.sort_by_key(|r| r.worst.rank());
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
        }
        for severity in [
            Severity::Ok,
            Severity::Warn,
            Severity::Crit,
            Severity::Stale,
            Severity::Missing,
        ] {
            assert!(
                sheets
                    .sheets
                    .iter()
                    .flat_map(|s| &s.rows)
                    .flat_map(|r| &r.cells)
                    .any(|c| c.severity == severity),
                "fixture lacks {severity:?}"
            );
        }
    }

    #[test]
    fn fixture_rows_are_ordered_by_severity_and_stable_within_rank() {
        let tables = tab_views(&fixture());
        let infra = &tables[0];
        assert_eq!(infra.title, "Infra");
        assert_eq!(infra.rows[0].id, "up:hyperliquid-exporter:localhost:9101");
        assert!(infra.rows[0].has_crit);
        let rest: Vec<_> = infra.rows[1..].iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            rest,
            [
                "up:pepers:localhost:8777",
                "hl:hl_blocks_behind{node=ws}",
                "up:workstation-node:localhost:9100"
            ]
        );
        assert_eq!(infra.crit_rows(), 1);
    }

    fn sheet_with_rows(rows: &[(&str, &[Severity])]) -> Sheet {
        Sheet {
            id: "s".into(),
            title: "S".into(),
            columns: vec!["a".into(), "b".into()],
            rows: rows
                .iter()
                .map(|(id, sev)| Row {
                    id: (*id).into(),
                    cells: sev
                        .iter()
                        .map(|&severity| Cell {
                            value: String::new(),
                            severity,
                            source: String::new(),
                            ts: None,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn rows_are_ordered_crit_warn_stale_missing_ok_stable_within_rank() {
        use Severity::*;
        let sheet = sheet_with_rows(&[
            ("ok1", &[Ok, Ok]),
            ("missing1", &[Ok, Missing]),
            ("stale1", &[Stale, Ok]),
            ("warn1", &[Ok, Warn]),
            ("crit1", &[Warn, Crit]),
            ("ok2", &[Ok]),
            ("warn2", &[Warn, Stale]),
            ("crit2", &[Crit, Missing]),
            ("empty", &[]),
        ]);
        let ids: Vec<_> = table_view(&sheet).rows.into_iter().map(|r| r.id).collect();
        assert_eq!(
            ids,
            [
                "crit1", "crit2", "warn1", "warn2", "stale1", "missing1", "ok1", "ok2", "empty"
            ]
        );
    }

    #[test]
    fn redact_url_strips_userinfo_query_and_fragment() {
        assert_eq!(
            redact_url("http://user:s3cret@host:8790/v1/sheets?token=abc#frag"),
            "http://host:8790/v1/sheets"
        );
        assert_eq!(redact_url("http://u@h/v1/sheets"), "http://h/v1/sheets");
        assert_eq!(
            redact_url("http://127.0.0.1:8790/v1/sheets"),
            "http://127.0.0.1:8790/v1/sheets"
        );
    }

    #[test]
    fn redact_url_never_echoes_an_unparsable_value() {
        let out = redact_url("http://user:pass@[not-an-ip");
        assert_eq!(out, "<invalid url>");
        assert!(!out.contains("pass"));
    }

    #[test]
    fn cell_severity_is_carried_to_the_view() {
        let tables = tab_views(&fixture());
        let exporter = &tables[0].rows[0];
        assert_eq!(exporter.cells[0].severity, Severity::Crit);
        assert_eq!(exporter.cells[1].text, "down");
        assert_eq!(exporter.cells[1].severity, Severity::Crit);
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
