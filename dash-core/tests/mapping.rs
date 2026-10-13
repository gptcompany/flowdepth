//! Pure JSON → cell mapping from recorded responses (no HTTP).

mod common;

use common::{fixture, now};
use dash_core::model::{Row, Severity};
use dash_core::sources::alerts::{self, AmAlert};
use dash_core::sources::prom::{self, PromReply};
use dash_core::sources::rektslug::{self, Health, Summary};

fn by_id<'a>(rows: &'a [Row], id: &str) -> &'a Row {
    rows.iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("row {id} missing in {rows:#?}"))
}

fn values(row: &Row) -> Vec<&str> {
    row.cells.iter().map(|c| c.value.as_str()).collect()
}

#[test]
fn prom_up_maps_ok_crit_stale() {
    let reply: PromReply = serde_json::from_str(&fixture("recorded/prom_up.json")).unwrap();
    let rows = prom::up_rows(reply, "http://prom/", now()).unwrap();
    assert_eq!(rows.len(), 3);

    let ok = by_id(&rows, "up:workstation-node:localhost:9100");
    assert_eq!(
        values(ok),
        [
            "workstation-node/localhost:9100",
            "up",
            "1",
            "http://prom/",
            "5"
        ]
    );
    assert_eq!(ok.cells[0].severity, Severity::Ok);
    assert_eq!(
        ok.cells[0].ts.unwrap().to_rfc3339(),
        "2026-10-13T11:59:55+00:00"
    );

    let down = by_id(&rows, "up:hyperliquid-exporter:localhost:9101");
    assert_eq!(values(down)[1..3], ["down", "0"]);
    assert!(down.cells.iter().all(|c| c.severity == Severity::Crit));

    let stale = by_id(&rows, "up:pepers:localhost:8777");
    assert_eq!(values(stale)[4], "3600");
    assert!(stale.cells.iter().all(|c| c.severity == Severity::Stale));
}

#[test]
fn prom_hl_series_one_row_each() {
    let reply: PromReply = serde_json::from_str(&fixture("recorded/prom_hl.json")).unwrap();
    let rows = prom::hl_rows(reply, "http://prom/", now()).unwrap();
    assert_eq!(rows.len(), 1);
    let r = by_id(&rows, "hl:hl_blocks_behind{node=ws}");
    assert_eq!(
        values(r)[..3],
        ["hl_blocks_behind{node=ws}", "present", "3"]
    );
}

#[test]
fn prom_error_status_is_reason_not_panic() {
    let reply: PromReply =
        serde_json::from_str(r#"{"status":"error","errorType":"bad_data","error":"parse error"}"#)
            .unwrap();
    assert_eq!(prom::up_rows(reply, "s", now()).unwrap_err(), "parse error");
}

#[test]
fn alerts_map_severity_label_and_staleness() {
    let alerts_in: Vec<AmAlert> =
        serde_json::from_str(&fixture("recorded/alertmanager_alerts.json")).unwrap();
    let rows = alerts::rows(alerts_in, "http://am/", now());
    assert_eq!(rows.len(), 4);

    let crit = by_id(&rows, "alert:a1b2c3d4e5f60001");
    assert_eq!(
        values(crit),
        [
            "HLNodeBehind (localhost:9101)",
            "critical",
            "Hyperliquid node 1200 blocks behind",
            "http://am/",
            "1800"
        ]
    );
    assert_eq!(crit.cells[0].severity, Severity::Crit);

    let warn = by_id(&rows, "alert:a1b2c3d4e5f60002");
    assert_eq!(warn.cells[0].severity, Severity::Warn);
    assert_eq!(values(warn)[2], "Disk 91% full");

    let unknown = by_id(&rows, "alert:a1b2c3d4e5f60003");
    assert_eq!(values(unknown)[1], "unknown");
    assert_eq!(unknown.cells[0].severity, Severity::Warn);

    // critical but updatedAt 30 min old: the datum is not fresh, so it must not read as live.
    let stale = by_id(&rows, "alert:a1b2c3d4e5f60004");
    assert_eq!(stale.cells[0].severity, Severity::Stale);
}

#[test]
fn rektslug_health_and_streams() {
    let health: Health = serde_json::from_str(&fixture("recorded/rektslug_health.json")).unwrap();
    let row = rektslug::health_row(&health, "http://rekt/", now());
    assert_eq!(values(&row)[..3], ["rektslug /health", "ok", "0.1.0"]);
    assert_eq!(row.cells[0].severity, Severity::Ok);

    let summary: Summary =
        serde_json::from_str(&fixture("recorded/rektslug_summary.json")).unwrap();
    let rows = rektslug::summary_rows(&summary, "http://rekt/", now());
    assert_eq!(rows.len(), 5);

    assert_eq!(
        by_id(&rows, "rektslug:ops").cells[0].severity,
        Severity::Warn
    );

    let fresh = by_id(&rows, "rektslug:klines:BTCUSDT");
    assert_eq!(values(fresh)[..2], ["klines:BTCUSDT", "fresh"]);
    assert_eq!(values(fresh)[2], "2026-10-13T11:59:00Z");
    assert_eq!(values(fresh)[4], "60");
    assert_eq!(fresh.cells[0].severity, Severity::Ok);

    assert_eq!(
        by_id(&rows, "rektslug:open_interest:BTCUSDT").cells[0].severity,
        Severity::Stale
    );

    let missing = by_id(&rows, "rektslug:funding_rates:BTCUSDT");
    assert_eq!(missing.cells[0].severity, Severity::Missing);
    assert_eq!(values(missing)[2], "no rows for symbol");
    assert!(missing.cells[0].ts.is_none());
}

#[test]
fn rektslug_unavailable_envelope_is_missing_with_reason() {
    let summary: Summary = serde_json::from_str(
        r#"{"status":"UNAVAILABLE","last_error":"questdb down","details":{}}"#,
    )
    .unwrap();
    let rows = rektslug::summary_rows(&summary, "s", now());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells[0].severity, Severity::Missing);
    assert_eq!(values(&rows[0])[2], "questdb down");
}
