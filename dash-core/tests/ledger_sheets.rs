//! Mapping of the `completeness` and `shadow` sheets (no HTTP).
//!
//! `recorded/prom_completeness.json` and `recorded/prom_shadow.json` are real replies of the
//! primary Prometheus (2026-10-26, labels trimmed to name/host/instance/job, timestamps moved to
//! the test epoch). The inline replies below are SYNTHETIC: on that date no
//! `hl_block_continuity_*` series and no ledger `nautilus_shadow_*` series exist yet, so their
//! shape follows the names fixed in gbg-agents#883.

mod common;

use common::{fixture, now};
use dash_core::model::{Row, Severity};
use dash_core::sources::completeness::{self, INACTIVE};
use dash_core::sources::prom::PromReply;
use dash_core::sources::shadow::{self, NO_DATA, UNRELIABLE};

fn by_id<'a>(rows: &'a [Row], id: &str) -> &'a Row {
    rows.iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("row {id} missing in {rows:#?}"))
}

fn cell(row: &Row, i: usize) -> (&str, Severity) {
    (row.cells[i].value.as_str(), row.cells[i].severity)
}

/// `(metric name, extra labels, value)` of one synthetic series.
type Series<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);

/// SYNTHETIC instant-query reply.
fn reply(ts: i64, series: &[Series]) -> PromReply {
    let result: Vec<_> = series
        .iter()
        .map(|(name, labels, v)| {
            let mut metric = serde_json::Map::new();
            metric.insert("__name__".into(), (*name).into());
            for (k, val) in *labels {
                metric.insert((*k).into(), (*val).into());
            }
            serde_json::json!({"metric": metric, "value": [ts, v]})
        })
        .collect();
    serde_json::from_value(serde_json::json!({
        "status": "success",
        "data": {"resultType": "vector", "result": result}
    }))
    .unwrap()
}

const NOW: i64 = 1_791_892_800;
const MIDNIGHT: i64 = 1_791_849_600;

#[test]
fn completeness_real_reply_says_detector_not_active() {
    let r: PromReply = serde_json::from_str(&fixture("recorded/prom_completeness.json")).unwrap();
    let rows = completeness::rows(r, "p", now()).unwrap();
    assert_eq!(rows.len(), 4);
    for node in ["tk-hl-1", "tk-hl-2"] {
        let h = by_id(&rows, &format!("completeness:{node}:hl_core_block_height"));
        assert_eq!(h.cells[1].value, "presente");
        assert_eq!(h.cells[0].severity, Severity::Ok);
        let c = by_id(&rows, &format!("completeness:{node}:hl_block_continuity"));
        assert_eq!(cell(c, 2), (INACTIVE, Severity::Missing));
    }
    let h = by_id(&rows, "completeness:tk-hl-1:hl_core_block_height");
    assert_eq!(h.cells[2].value, "1197638346");
}

#[test]
fn completeness_active_detector_flags_gaps_and_absent_series() {
    let n: &[(&str, &str)] = &[("host", "n1")];
    let run = (NOW - 120).to_string();
    let r = reply(
        NOW,
        &[
            ("hl_core_block_height", n, "100"),
            ("hl_block_continuity_last_block", n, "99"),
            ("hl_block_continuity_gap_blocks_total", n, "3"),
            ("hl_block_continuity_last_gap_start", n, "50"),
            ("hl_block_continuity_run_timestamp_seconds", n, &run),
        ],
    );
    let rows = completeness::rows(r, "p", now()).unwrap();
    let get = |m: &str| by_id(&rows, &format!("completeness:n1:{m}"));
    assert_eq!(
        cell(get("hl_block_continuity_last_block"), 2),
        ("99", Severity::Ok)
    );
    assert_eq!(
        cell(get("hl_block_continuity_gap_blocks_total"), 2),
        ("3", Severity::Warn)
    );
    assert_eq!(
        cell(get("hl_block_continuity_last_gap_end"), 2),
        ("assente", Severity::Missing)
    );
    let run = get("hl_block_continuity_run_timestamp_seconds");
    assert_eq!(cell(run, 2), ("2 min", Severity::Ok));
    assert_eq!(run.cells[4].value, "120");
}

#[test]
fn completeness_old_detector_run_is_never_ok() {
    let n: &[(&str, &str)] = &[("instance", "n2")];
    let run = (NOW - 3600).to_string();
    let r = reply(
        NOW,
        &[
            ("hl_block_continuity_last_block", n, "99"),
            ("hl_block_continuity_gap_blocks_total", n, "0"),
            ("hl_block_continuity_run_timestamp_seconds", n, &run),
        ],
    );
    let rows = completeness::rows(r, "p", now()).unwrap();
    let run = by_id(
        &rows,
        "completeness:n2:hl_block_continuity_run_timestamp_seconds",
    );
    assert_eq!(run.cells[1].value, "vecchio di 60 minuti");
    for row in rows.iter().filter(|r| r.id.contains("continuity")) {
        assert!(
            row.cells
                .iter()
                .all(|c| matches!(c.severity, Severity::Stale | Severity::Missing)),
            "{row:#?}"
        );
    }
}

#[test]
fn shadow_real_reply_says_no_ledger_data() {
    let r: PromReply = serde_json::from_str(&fixture("recorded/prom_shadow.json")).unwrap();
    let empty: PromReply = serde_json::from_str(&fixture("recorded/prom_shadow.json")).unwrap();
    assert_eq!(shadow::rows(r, Ok(empty), "p", now()).unwrap_err(), NO_DATA);
}

#[test]
fn shadow_ignores_other_nautilus_shadow_producers() {
    // Names really present in Prometheus on 2026-10-26, from the health/probe exporter.
    let s: &[(&str, &str)] = &[("strategy", "x")];
    let r = reply(
        NOW,
        &[
            ("nautilus_shadow_health_up", s, "1"),
            ("nautilus_shadow_hl_latency_gate_ok", s, "1"),
            ("nautilus_shadow_stream_trades", s, "42"),
        ],
    );
    let empty = reply(MIDNIGHT, &[]);
    assert_eq!(shadow::rows(r, Ok(empty), "p", now()).unwrap_err(), NO_DATA);
}

fn ledger(run_age_s: i64) -> PromReply {
    let a: &[(&str, &str)] = &[("strategy", "a")];
    let buy: &[(&str, &str)] = &[("strategy", "a"), ("side", "buy"), ("status", "filled")];
    let sell: &[(&str, &str)] = &[("strategy", "a"), ("side", "sell"), ("status", "rejected")];
    let run = (NOW - run_age_s).to_string();
    let last = (NOW - 600).to_string();
    reply(
        NOW,
        &[
            ("nautilus_shadow_orders_total", buy, "7"),
            ("nautilus_shadow_orders_total", sell, "3"),
            ("nautilus_shadow_fills_total", a, "7"),
            ("nautilus_shadow_realized_pnl_usd", a, "-1.5"),
            ("nautilus_shadow_last_order_timestamp_seconds", a, &last),
            ("nautilus_shadow_exporter_run_timestamp_seconds", &[], &run),
        ],
    )
}

#[test]
fn shadow_ledger_rows_with_fresh_exporter() {
    let b: &[(&str, &str)] = &[("strategy", "a"), ("side", "buy"), ("status", "filled")];
    let midnight = reply(MIDNIGHT, &[("nautilus_shadow_orders_total", b, "4")]);
    let rows = shadow::rows(ledger(60), Ok(midnight), "p", now()).unwrap();
    let get = |k: &str| by_id(&rows, &format!("shadow:a:{k}"));
    assert_eq!(cell(get("ordini simulati totali"), 2), ("10", Severity::Ok));
    assert_eq!(cell(get("ordini simulati oggi"), 2), ("6", Severity::Ok));
    assert_eq!(cell(get("fill"), 2), ("7", Severity::Ok));
    assert_eq!(cell(get("ultimo ordine"), 2), ("10 min fa", Severity::Ok));
    let pnl = get("PnL realizzato simulato");
    assert_eq!(pnl.cells[1].value, UNRELIABLE);
    assert_eq!(cell(pnl, 2), ("-1.50 USD (non affidabile)", Severity::Warn));
    assert_eq!(
        cell(by_id(&rows, "shadow:exporter"), 2),
        ("1 min", Severity::Ok)
    );
}

#[test]
fn shadow_old_exporter_makes_every_row_stale() {
    let rows = shadow::rows(ledger(3600), Err("http 503".into()), "p", now()).unwrap();
    assert_eq!(
        by_id(&rows, "shadow:exporter").cells[1].value,
        "vecchio di 60 minuti"
    );
    assert_eq!(
        cell(by_id(&rows, "shadow:a:ordini simulati oggi"), 2),
        ("assente: http 503", Severity::Missing)
    );
    for row in &rows {
        assert!(
            row.cells
                .iter()
                .all(|c| matches!(c.severity, Severity::Stale | Severity::Missing)),
            "{row:#?}"
        );
    }
}

#[test]
fn shadow_orders_today_without_midnight_sample_or_after_reset() {
    let rows = shadow::rows(ledger(60), Ok(reply(MIDNIGHT, &[])), "p", now()).unwrap();
    assert_eq!(
        cell(by_id(&rows, "shadow:a:ordini simulati oggi"), 2),
        ("assente: nessun campione alle 00:00Z", Severity::Missing)
    );
    let b: &[(&str, &str)] = &[("strategy", "a"), ("side", "buy"), ("status", "filled")];
    let midnight = reply(MIDNIGHT, &[("nautilus_shadow_orders_total", b, "50")]);
    let rows = shadow::rows(ledger(60), Ok(midnight), "p", now()).unwrap();
    let today = by_id(&rows, "shadow:a:ordini simulati oggi");
    assert_eq!(today.cells[1].value, "contatore azzerato oggi");
    assert_eq!(cell(today, 2), ("assente", Severity::Warn));
}

#[test]
fn midnight_is_start_of_utc_day() {
    assert_eq!(shadow::midnight_utc(now()), MIDNIGHT);
}
