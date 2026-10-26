//! Reproducer of review rev-fd4-r4 R1 (flowdepth#4): a fresh detector run with missing blocks
//! must keep its known age and not blame freshness. SYNTHETIC replies, as in `ledger_sheets.rs`.

use dash_core::model::Severity;
use dash_core::sources::{completeness, prom::PromReply};

#[test]
fn fresh_gap_has_known_source_age() {
    let now = chrono::DateTime::from_timestamp(1_791_892_800, 0).unwrap();
    let samples: PromReply = serde_json::from_value(serde_json::json!({
        "status": "success", "data": {"resultType": "vector", "result": [
            {"metric": {"__name__": "hl_block_continuity_run_timestamp_seconds", "host": "node"}, "value": [1_791_892_800, "1791892680"]},
            {"metric": {"__name__": "hl_block_continuity_gap_blocks_total", "host": "node"}, "value": [1_791_892_800, "3"]}
        ]}
    })).unwrap();
    let rows = completeness::rows(samples, "test", now).unwrap();
    let row = rows
        .iter()
        .find(|r| r.id == "completeness:node:hl_block_continuity_gap_blocks_total")
        .unwrap();
    println!(
        "status={} value={} age_s={} ts={:?} severity={:?}",
        row.cells[1].value,
        row.cells[2].value,
        row.cells[4].value,
        row.cells[1].ts,
        row.cells[1].severity
    );
    assert_eq!(row.cells[1].severity, Severity::Warn);
    assert_eq!(row.cells[4].value, "120");
    assert!(row.cells[1].ts.is_some());
    assert_ne!(
        row.cells[1].value, "età della fonte ignota",
        "fresh gap row must not claim unknown source age"
    );
}

#[test]
fn zero_gaps_with_the_same_known_age_are_present() {
    let now = chrono::DateTime::from_timestamp(1_791_892_800, 0).unwrap();
    let samples: PromReply = serde_json::from_value(serde_json::json!({
        "status": "success", "data": {"resultType": "vector", "result": [
            {"metric": {"__name__": "hl_block_continuity_run_timestamp_seconds", "host": "node"}, "value": [1_791_892_800, "1791892680"]},
            {"metric": {"__name__": "hl_block_continuity_gap_blocks_total", "host": "node"}, "value": [1_791_892_800, "0"]}
        ]}
    })).unwrap();
    let rows = completeness::rows(samples, "test", now).unwrap();
    let row = rows
        .iter()
        .find(|r| r.id == "completeness:node:hl_block_continuity_gap_blocks_total")
        .unwrap();
    println!(
        "CONTROL status={} value={} age_s={} ts={:?} severity={:?}",
        row.cells[1].value,
        row.cells[2].value,
        row.cells[4].value,
        row.cells[1].ts,
        row.cells[1].severity
    );
    assert_eq!(row.cells[1].value, "presente");
    assert_eq!(row.cells[1].severity, Severity::Ok);
    assert_eq!(row.cells[4].value, "120");
}
