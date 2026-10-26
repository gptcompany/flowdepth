//! Mapping of the `completeness` and `shadow` sheets (no HTTP).
//!
//! `recorded/prom_completeness.json` and `recorded/prom_shadow.json` are real replies of the
//! primary Prometheus (2026-10-26, labels trimmed to name/host/instance/job, sample timestamps
//! moved to the test epoch and `hl_core_latest_block_time` shifted by the same offset, so block
//! ages are the real ones). The inline replies below are SYNTHETIC: on that date no
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
        let c = by_id(&rows, &format!("completeness:{node}:hl_block_continuity"));
        assert_eq!(cell(c, 2), (INACTIVE, Severity::Missing));
    }
    let h = by_id(&rows, "completeness:tk-hl-1:hl_core_block_height");
    assert_eq!(h.cells[1].value, "aggiornato");
    assert_eq!(cell(h, 2), ("1197650826", Severity::Ok));
    assert_eq!(h.cells[4].value, "29");
}

#[test]
fn completeness_stuck_node_height_is_stale_not_ok() {
    // Real: on 2026-10-26 tk-hl-2 kept exporting the same height, its latest block ~44 h old.
    let r: PromReply = serde_json::from_str(&fixture("recorded/prom_completeness.json")).unwrap();
    let rows = completeness::rows(r, "p", now()).unwrap();
    let h = by_id(&rows, "completeness:tk-hl-2:hl_core_block_height");
    assert_eq!(h.cells[1].value, "vecchio di 2642 minuti");
    assert_eq!(cell(h, 2), ("1195454145", Severity::Stale));
    assert!(h.cells.iter().all(|c| c.severity == Severity::Stale));
}

#[test]
fn completeness_height_without_block_time_is_never_ok() {
    let n: &[(&str, &str)] = &[("host", "n3")];
    let rows =
        completeness::rows(reply(NOW, &[("hl_core_block_height", n, "7")]), "p", now()).unwrap();
    let h = by_id(&rows, "completeness:n3:hl_core_block_height");
    assert_eq!(cell(h, 2), ("7", Severity::Warn));
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
    let gaps = get("hl_block_continuity_gap_blocks_total");
    assert_eq!(cell(gaps, 2), ("3", Severity::Warn));
    // The warning is about the gap, not the run's age, which is known (R1, rev-fd4-r4).
    assert_eq!(gaps.cells[1].value, completeness::GAPS_STATUS);
    assert_eq!(gaps.cells[4].value, "120");
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
    assert_eq!(
        shadow::rows(r, Ok(empty), Ok(no_resets()), "p", now()).unwrap_err(),
        NO_DATA
    );
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
    assert_eq!(
        shadow::rows(r, Ok(empty), Ok(no_resets()), "p", now()).unwrap_err(),
        NO_DATA
    );
}

/// Status label values come from the single constant, so confirming them touches one place.
const ENTRY: &str = shadow::ORDER_STATUSES[0].0;
const DENIED: &str = shadow::ORDER_STATUSES[1].0;
const FILLED: &str = shadow::ORDER_STATUSES[2].0;
const BLOCKED: &str = shadow::ORDER_STATUSES[3].0;

fn lbl<'a>(side: &'a str, status: &'a str) -> [(&'a str, &'a str); 3] {
    [("strategy", "a"), ("side", side), ("status", status)]
}

/// SYNTHETIC ledger. `orders_total` counts orders that reached each status, once per status:
/// 10 entered (6 buy + 4 sell), 2 denied, 7 filled, 3 blocked (side always UNKNOWN), plus 1
/// blocked without an order id.
fn ledger_with(run_age_s: i64, entry_sell: &str) -> PromReply {
    ledger_series(run_age_s, entry_sell, true)
}

fn ledger_series(run_age_s: i64, entry_sell: &str, without_id: bool) -> PromReply {
    let a: &[(&str, &str)] = &[("strategy", "a")];
    let run = (NOW - run_age_s).to_string();
    let last = (NOW - 600).to_string();
    let (be, se, bd, bf, sf, ub) = (
        lbl("buy", ENTRY),
        lbl("sell", ENTRY),
        lbl("buy", DENIED),
        lbl("buy", FILLED),
        lbl("sell", FILLED),
        lbl("UNKNOWN", BLOCKED),
    );
    let mut series: Vec<Series> = vec![
        ("nautilus_shadow_orders_total", &be, "6"),
        ("nautilus_shadow_orders_total", &se, entry_sell),
        ("nautilus_shadow_orders_total", &bd, "2"),
        ("nautilus_shadow_orders_total", &bf, "5"),
        ("nautilus_shadow_orders_total", &sf, "2"),
        ("nautilus_shadow_orders_total", &ub, "3"),
        ("nautilus_shadow_fills_total", a, "7"),
        ("nautilus_shadow_realized_pnl_usd", a, "-1.5"),
        ("nautilus_shadow_last_order_timestamp_seconds", a, &last),
        ("nautilus_shadow_exporter_run_timestamp_seconds", &[], &run),
    ];
    if without_id {
        series.push(("nautilus_shadow_blocked_without_id_total", a, "1"));
    }
    reply(NOW, &series)
}

fn ledger(run_age_s: i64) -> PromReply {
    ledger_with(run_age_s, "4")
}

/// SYNTHETIC `orders_total` at 00:00Z: `entry` entered (buy) plus `entry_sell` if given,
/// 1 denied, 3 filled, 1 blocked.
fn baseline_with(entry: &str, entry_sell: Option<&str>) -> PromReply {
    let (buy_e, sell_e, buy_d, buy_f, unk_b) = (
        lbl("buy", ENTRY),
        lbl("sell", ENTRY),
        lbl("buy", DENIED),
        lbl("buy", FILLED),
        lbl("UNKNOWN", BLOCKED),
    );
    let mut series: Vec<Series> = vec![
        ("nautilus_shadow_orders_total", &buy_e, entry),
        ("nautilus_shadow_orders_total", &buy_d, "1"),
        ("nautilus_shadow_orders_total", &buy_f, "3"),
        ("nautilus_shadow_orders_total", &unk_b, "1"),
    ];
    if let Some(v) = entry_sell {
        series.push(("nautilus_shadow_orders_total", &sell_e, v));
    }
    reply(MIDNIGHT, &series)
}

fn baseline(entry: &str) -> PromReply {
    baseline_with(entry, None)
}

/// SYNTHETIC `sum by (strategy, status) (resets(...))` reply; aggregated, so no `__name__`.
fn resets(entry: &str) -> PromReply {
    let st = |status| [("strategy", "a"), ("status", status)];
    reply(
        NOW,
        &[
            ("", &st(ENTRY), entry),
            ("", &st(DENIED), "0"),
            ("", &st(FILLED), "0"),
            ("", &st(BLOCKED), "0"),
        ],
    )
}

fn no_resets() -> PromReply {
    resets("0")
}

fn get<'a>(rows: &'a [Row], k: &str) -> &'a Row {
    by_id(rows, &format!("shadow:a:{k}"))
}

#[test]
fn shadow_statuses_are_separate_columns_never_summed() {
    let rows = shadow::rows(ledger(60), Ok(baseline("4")), Ok(no_resets()), "p", now()).unwrap();
    for (k, v) in [
        ("ordini simulati totali", "10"),
        ("ordini simulati oggi", "6"),
        ("denied totali", "2"),
        ("denied oggi", "1"),
        ("filled totali", "7"),
        ("filled oggi", "4"),
        ("bloccati totali", "3"),
        ("bloccati oggi", "2"),
        ("bloccati senza id", "1"),
        ("fill", "7"),
        ("ultimo ordine", "10 min fa"),
    ] {
        assert_eq!(cell(get(&rows, k), 2), (v, Severity::Ok), "{k}");
    }
    let pnl = get(&rows, "PnL realizzato simulato");
    assert_eq!(pnl.cells[1].value, UNRELIABLE);
    assert_eq!(cell(pnl, 2), ("-1.50 USD (non affidabile)", Severity::Warn));
    assert_eq!(
        cell(by_id(&rows, "shadow:exporter"), 2),
        ("1 min", Severity::Ok)
    );
}

#[test]
fn shadow_old_exporter_makes_every_row_stale() {
    let rows = shadow::rows(
        ledger(3600),
        Err("http 503".into()),
        Ok(no_resets()),
        "p",
        now(),
    )
    .unwrap();
    assert_eq!(
        by_id(&rows, "shadow:exporter").cells[1].value,
        "vecchio di 60 minuti"
    );
    assert_eq!(
        cell(get(&rows, "ordini simulati oggi"), 2),
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
fn shadow_orders_today_without_midnight_sample_or_below_baseline() {
    let rows = shadow::rows(
        ledger(60),
        Ok(reply(MIDNIGHT, &[])),
        Ok(no_resets()),
        "p",
        now(),
    )
    .unwrap();
    assert_eq!(
        cell(get(&rows, "ordini simulati oggi"), 2),
        ("assente: nessun campione alle 00:00Z", Severity::Missing)
    );
    let rows = shadow::rows(ledger(60), Ok(baseline("50")), Ok(no_resets()), "p", now()).unwrap();
    assert_eq!(
        cell(get(&rows, "ordini simulati oggi"), 2),
        ("non valutabile: contatore azzerato", Severity::Warn)
    );
}

#[test]
fn shadow_reset_since_midnight_is_not_assessable_even_when_total_grew() {
    // Baseline 4 at 00:00Z, reset afterwards, then 10 entered: "10 - 4 = 6" would be fake.
    let rows = shadow::rows(ledger(60), Ok(baseline("4")), Ok(resets("1")), "p", now()).unwrap();
    let today = get(&rows, "ordini simulati oggi");
    assert_eq!(today.cells[1].value, "non valutabile");
    assert_eq!(
        cell(today, 2),
        ("non valutabile: contatore azzerato", Severity::Warn)
    );
    // The reset is per status: the other columns keep their difference; totals stay as reported.
    assert_eq!(cell(get(&rows, "filled oggi"), 2), ("4", Severity::Ok));
    assert_eq!(
        cell(get(&rows, "ordini simulati totali"), 2),
        ("10", Severity::Ok)
    );
}

#[test]
fn shadow_partial_midnight_baseline_or_failed_reset_check_is_never_ok() {
    let partial = baseline_with("4", Some("+Inf"));
    let rows = shadow::rows(ledger(60), Ok(partial), Ok(no_resets()), "p", now()).unwrap();
    assert_eq!(
        cell(get(&rows, "ordini simulati oggi"), 2),
        ("non valutabile: serie non numerica", Severity::Warn)
    );
    let rows = shadow::rows(
        ledger(60),
        Ok(baseline("4")),
        Err("http 503".into()),
        "p",
        now(),
    )
    .unwrap();
    assert_eq!(
        cell(get(&rows, "denied oggi"), 2),
        ("assente: http 503", Severity::Missing)
    );
}

#[test]
fn shadow_non_finite_series_is_not_assessable_never_ok() {
    let rows = shadow::rows(
        ledger_with(60, "NaN"),
        Ok(baseline("4")),
        Ok(no_resets()),
        "p",
        now(),
    )
    .unwrap();
    for k in ["ordini simulati totali", "ordini simulati oggi"] {
        let row = get(&rows, k);
        assert_eq!(row.cells[1].value, "non valutabile", "{row:#?}");
        assert_eq!(row.cells[0].severity, Severity::Warn);
    }
}

#[test]
fn shadow_missing_blocked_without_id_series_is_absent_not_zero() {
    let r = ledger_series(60, "4", false);
    let rows = shadow::rows(r, Ok(baseline("4")), Ok(no_resets()), "p", now()).unwrap();
    assert_eq!(
        cell(get(&rows, "bloccati senza id"), 2),
        ("assente", Severity::Missing)
    );
}

#[test]
fn resets_window_starts_at_utc_midnight() {
    assert_eq!(
        shadow::resets_query(now()),
        "sum by (strategy, status) (resets(nautilus_shadow_orders_total[43500s]))"
    );
}

#[test]
fn resets_window_covers_a_baseline_sample_taken_before_midnight() {
    // Codex r2: baseline 4 at 23:59:55, reset, 0 at 00:00:05, 10 at 00:00:30, read at 00:01:00.
    // A window starting at 00:00Z (60s) misses the reset; the one used must reach 23:59:55.
    let at = |t: &str| t.parse::<chrono::DateTime<chrono::Utc>>().unwrap();
    let q = shadow::resets_query(at("2026-10-14T00:01:00Z"));
    let window: i64 = q
        .rsplit_once('[')
        .and_then(|(_, w)| w.strip_suffix("s]))"))
        .unwrap()
        .parse()
        .unwrap();
    let start = at("2026-10-14T00:01:00Z").timestamp() - window;
    assert!(start <= at("2026-10-13T23:59:55Z").timestamp(), "{q}");
    // Whatever the sample spacing, the window reaches the oldest baseline the lookback allows.
    assert_eq!(
        start,
        at("2026-10-14T00:00:00Z").timestamp() - shadow::LOOKBACK_S
    );
}

#[test]
fn midnight_is_start_of_utc_day() {
    assert_eq!(shadow::midnight_utc(now()), MIDNIGHT);
}
