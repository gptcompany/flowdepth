//! Prometheus → sheet "completeness": per HL node, the block-continuity detector series
//! (`hl_block_continuity_*`, hyperliquid-node#194) next to `hl_core_block_height`.
//!
//! A node exporting the height but no continuity series reads «rilevatore non attivo»: the
//! detector is not deployed there, which is not the same as "no gaps".

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use super::prom::{PromReply, PromSample, label, samples};
use super::{derived, endpoint_url, finite, run_freshness, unavailable_sheet};
use crate::config::Endpoint;
use crate::fetch::Fetcher;
use crate::model::{Reading, Row, Severity, Sheet};

const SOURCE: &str = "prometheus";
const SELECTOR: &str = r#"{__name__=~"hl_block_continuity_(last_block|gap_blocks_total|last_gap_start|last_gap_end|run_timestamp_seconds)|hl_core_block_height|hl_core_latest_block_time"}"#;
const HEIGHT: &str = "hl_core_block_height";
/// Unix seconds of the node's latest block (checked against the live series on 2026-10-26).
const BLOCK_TIME: &str = "hl_core_latest_block_time";
const RUN_TS: &str = "hl_block_continuity_run_timestamp_seconds";
const GAPS: &str = "hl_block_continuity_gap_blocks_total";
/// Continuity series shown per node once the detector is active.
const CONTINUITY: [(&str, &str); 4] = [
    ("hl_block_continuity_last_block", "ultimo blocco verificato"),
    (GAPS, "blocchi mancanti (totale)"),
    ("hl_block_continuity_last_gap_start", "ultimo buco: inizio"),
    ("hl_block_continuity_last_gap_end", "ultimo buco: fine"),
];
/// A detector run older than this makes the node's continuity rows stale. The detector cadence
/// is not published yet (hyperliquid-node#194); 15 minutes leaves room for a 5-minute timer.
pub const CONTINUITY_STALE_S: i64 = 900;
/// A latest block older than this makes the node's height stale: a stuck node keeps exporting
/// the same height on every scrape, so the sample age alone would read `ok`. HL produces blocks
/// sub-second; 15 minutes only flags a node that has stopped, not one that lags.
pub const BLOCK_STALE_S: i64 = 900;
pub const INACTIVE: &str = "rilevatore non attivo";
pub const GAPS_STATUS: &str = "blocchi mancanti";

/// Rows for one node: height, then either «rilevatore non attivo» or run age plus each series.
fn node_rows(
    node: &str,
    series: &BTreeMap<&str, &PromSample>,
    now: DateTime<Utc>,
) -> Vec<(String, Reading)> {
    let value = |m: &str| series.get(m).and_then(|s| finite(&s.value.1));
    let comp = |title: &str| format!("{node} · {title}");
    // The height is as fresh as the node's latest block, not as the scrape that carried it.
    let (fresh, block) = run_freshness(
        comp("altezza blocco"),
        value(BLOCK_TIME),
        now,
        BLOCK_STALE_S,
    );
    let height = match value(HEIGHT) {
        Some(v) if block.ts.is_some() => Reading {
            value: format!("{v:.0}"),
            ..block
        },
        Some(v) => derived(&block, fresh, comp("altezza blocco"), format!("{v:.0}")),
        None => Reading::missing(&comp("altezza blocco"), "", "assente"),
    };
    let mut out = vec![(format!("completeness:{node}:{HEIGHT}"), height)];

    if !series.keys().any(|k| k.starts_with("hl_block_continuity_")) {
        let r = Reading::missing(&comp("continuità blocchi"), "", INACTIVE);
        out.push((format!("completeness:{node}:hl_block_continuity"), r));
        return out;
    }
    // The detector run decides whether its values may read as current.
    let (base, run) = run_freshness(
        comp("ultimo giro rilevatore"),
        value(RUN_TS),
        now,
        CONTINUITY_STALE_S,
    );
    for (metric, title) in CONTINUITY {
        let r = match value(metric) {
            // Missing blocks are a completeness defect, even on a fresh run: the status names
            // that cause, while ts/age stay the run's.
            Some(v) if metric == GAPS && v > 0.0 && base == Severity::Ok => Reading {
                status: GAPS_STATUS.into(),
                ..derived(&run, Severity::Warn, comp(title), format!("{v:.0}"))
            },
            Some(v) => derived(&run, base, comp(title), format!("{v:.0}")),
            None => Reading::missing(&comp(title), "", "assente"),
        };
        out.push((format!("completeness:{node}:{metric}"), r));
    }
    out.push((format!("completeness:{node}:{RUN_TS}"), run));
    out
}

pub fn rows(reply: PromReply, source: &str, now: DateTime<Utc>) -> Result<Vec<Row>, String> {
    let samples = samples(reply)?;
    // Node = `host`, else `instance`: the label the HL exporters already use for the node.
    let mut nodes: BTreeMap<&str, BTreeMap<&str, &PromSample>> = BTreeMap::new();
    for s in &samples {
        let name = label(s, "__name__");
        if ![HEIGHT, BLOCK_TIME, RUN_TS].contains(&name)
            && !CONTINUITY.iter().any(|(m, _)| *m == name)
        {
            continue;
        }
        let node = s
            .metric
            .get("host")
            .or_else(|| s.metric.get("instance"))
            .map_or("?", String::as_str);
        nodes.entry(node).or_default().insert(name, s);
    }
    if nodes.is_empty() {
        return Err("nessuna serie hl_core_block_height o hl_block_continuity_*".to_owned());
    }
    Ok(nodes
        .iter()
        .flat_map(|(node, series)| node_rows(node, series, now))
        .map(|(id, r)| {
            Reading {
                source: source.to_owned(),
                ..r
            }
            .into_row(id)
        })
        .collect())
}

pub async fn sheet(fetcher: &Fetcher, ep: &Endpoint, now: DateTime<Utc>) -> Sheet {
    const ID: &str = "completeness";
    const TITLE: &str = "Completezza";
    if let Some(sheet) = unavailable_sheet(ID, TITLE, "completeness:prometheus", SOURCE, ep) {
        return sheet;
    }
    let Endpoint::Ready(base) = ep else {
        unreachable!("unavailable_sheet covers non-ready endpoints")
    };
    let label = ep.label();
    let url = endpoint_url(base, "api/v1/query", &[("query", SELECTOR)]);
    let rows = fetcher
        .get_json::<PromReply>(SOURCE, url)
        .await
        .and_then(|r| rows(r, &label, now))
        .unwrap_or_else(|reason| {
            let r = Reading::missing(
                "hl_core_block_height, hl_block_continuity_*",
                &label,
                &format!("assente: {reason}"),
            );
            vec![r.into_row("completeness:prometheus")]
        });
    Sheet::new(ID, TITLE, rows)
}
