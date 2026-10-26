//! Prometheus → sheet "shadow": per Nautilus shadow strategy, the series written by the ledger
//! exporter (gbg-agents#883 step 5).
//!
//! Only the exporter's exact metric names are selected and mapped: Prometheus also holds
//! `nautilus_shadow_health_*` and `nautilus_shadow_hl_*` from another producer, which say nothing
//! about simulated orders.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use super::prom::{PromReply, label, samples};
use super::{derived, endpoint_url, finite, run_freshness, unavailable_sheet};
use crate::config::Endpoint;
use crate::fetch::Fetcher;
use crate::model::{Reading, Row, Severity, Sheet, age_secs};

const SOURCE: &str = "prometheus";
const SELECTOR: &str = r#"{__name__=~"nautilus_shadow_(orders_total|fills_total|realized_pnl_usd|last_order_timestamp_seconds|exporter_run_timestamp_seconds)"}"#;
const ORDERS: &str = "nautilus_shadow_orders_total";
const RUN_TS: &str = "nautilus_shadow_exporter_run_timestamp_seconds";
/// An exporter run older than this makes every ledger row stale. The exporter timer is not
/// deployed yet; 15 minutes leaves room for a 5-minute timer.
pub const EXPORTER_STALE_S: i64 = 900;
pub const NO_DATA: &str = "nessun dato dal ledger shadow";
/// Shown on the PnL until nautilus_dev F-033/F-035 are closed in its findings register.
pub const UNRELIABLE: &str = "non affidabile";

#[derive(Default)]
struct Strategy {
    orders: Option<f64>,
    fills: Option<f64>,
    pnl: Option<f64>,
    last_order: Option<f64>,
}

/// `orders_total` summed over `side` and `status` per strategy.
fn orders_by_strategy(reply: PromReply) -> Result<BTreeMap<String, f64>, String> {
    let mut out = BTreeMap::new();
    for s in samples(reply)? {
        if let (ORDERS, Some(v)) = (label(&s, "__name__"), finite(&s.value.1)) {
            *out.entry(label(&s, "strategy").to_owned()).or_insert(0.0) += v;
        }
    }
    Ok(out)
}

/// `now_reply` is the current ledger state; `midnight` holds `orders_total` at 00:00Z of `now`'s
/// day, from which "orders today" is the difference. A failed midnight query only blanks that row.
pub fn rows(
    now_reply: PromReply,
    midnight: Result<PromReply, String>,
    source: &str,
    now: DateTime<Utc>,
) -> Result<Vec<Row>, String> {
    let mut strategies: BTreeMap<String, Strategy> = BTreeMap::new();
    let mut run = None;
    for s in samples(now_reply)? {
        let (name, v) = (label(&s, "__name__"), finite(&s.value.1));
        if name == RUN_TS {
            run = v;
            continue;
        }
        // Only the exporter's names open a strategy: other producers share the prefix.
        let slot: fn(&mut Strategy) -> &mut Option<f64> = match name {
            ORDERS => |st| &mut st.orders,
            "nautilus_shadow_fills_total" => |st| &mut st.fills,
            "nautilus_shadow_realized_pnl_usd" => |st| &mut st.pnl,
            "nautilus_shadow_last_order_timestamp_seconds" => |st| &mut st.last_order,
            _ => continue,
        };
        let Some(v) = v else { continue };
        let st = strategies
            .entry(label(&s, "strategy").to_owned())
            .or_default();
        // `orders_total` has one series per side and status: sum them.
        let acc = slot(st);
        *acc = Some(acc.unwrap_or(0.0) + v);
    }
    if run.is_none() && strategies.is_empty() {
        return Err(NO_DATA.to_owned());
    }

    let (base, exporter) = run_freshness("esportatore ledger".into(), run, now, EXPORTER_STALE_S);
    let at_midnight = midnight.and_then(orders_by_strategy);
    let mut out = Vec::new();
    for (name, st) in &strategies {
        let comp = |k: &str| format!("{name} · {k}");
        let push = |out: &mut Vec<(String, Reading)>, k: &str, r: Reading| {
            out.push((format!("shadow:{name}:{k}"), r));
        };
        let count = |k: &str, v: Option<f64>| match v {
            Some(v) => derived(&exporter, base, comp(k), format!("{v:.0}")),
            None => Reading::missing(&comp(k), "", "assente"),
        };
        push(
            &mut out,
            "ordini simulati totali",
            count("ordini simulati totali", st.orders),
        );
        push(&mut out, "fill", count("fill", st.fills));

        let k = "ordini simulati oggi";
        let today = match (st.orders, &at_midnight) {
            (None, _) => Reading::missing(&comp(k), "", "assente"),
            (Some(_), Err(reason)) => Reading::missing(&comp(k), "", &format!("assente: {reason}")),
            (Some(total), Ok(m)) => match m.get(name) {
                None => Reading::missing(&comp(k), "", "assente: nessun campione alle 00:00Z"),
                Some(start) if total < *start => Reading {
                    status: "contatore azzerato oggi".into(),
                    severity: Severity::Warn,
                    ..Reading::missing(&comp(k), "", "assente")
                },
                Some(start) => count(k, Some(total - start)),
            },
        };
        push(&mut out, k, today);

        let k = "ultimo ordine";
        let last = match st
            .last_order
            .and_then(|v| DateTime::from_timestamp(v as i64, 0))
        {
            Some(t) => Reading {
                ts: Some(t),
                age_s: Some(age_secs(t, now)),
                ..derived(
                    &exporter,
                    base,
                    comp(k),
                    format!("{} min fa", age_secs(t, now) / 60),
                )
            },
            None => Reading::missing(&comp(k), "", "assente"),
        };
        push(&mut out, k, last);

        // Simulated PnL is never `ok` while the accounting findings are open.
        let k = "PnL realizzato simulato";
        let pnl = match st.pnl {
            Some(v) => Reading {
                status: UNRELIABLE.into(),
                severity: if base == Severity::Ok {
                    Severity::Warn
                } else {
                    base
                },
                ..derived(
                    &exporter,
                    base,
                    comp(k),
                    format!("{v:.2} USD ({UNRELIABLE})"),
                )
            },
            None => Reading::missing(&comp(k), "", "assente"),
        };
        push(&mut out, k, pnl);
    }
    out.push(("shadow:exporter".to_owned(), exporter));
    Ok(out
        .into_iter()
        .map(|(id, r)| {
            Reading {
                source: source.to_owned(),
                ..r
            }
            .into_row(id)
        })
        .collect())
}

/// Unix seconds of 00:00Z on `now`'s UTC day.
pub fn midnight_utc(now: DateTime<Utc>) -> i64 {
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .map_or(0, |m| m.and_utc().timestamp())
}

pub async fn sheet(fetcher: &Fetcher, ep: &Endpoint, now: DateTime<Utc>) -> Sheet {
    const ID: &str = "shadow";
    const TITLE: &str = "Shadow";
    if let Some(sheet) = unavailable_sheet(ID, TITLE, "shadow:prometheus", SOURCE, ep) {
        return sheet;
    }
    let Endpoint::Ready(base) = ep else {
        unreachable!("unavailable_sheet covers non-ready endpoints")
    };
    let label = ep.label();
    let midnight = midnight_utc(now).to_string();
    let (current, at_midnight) = tokio::join!(
        fetcher.get_json::<PromReply>(
            SOURCE,
            endpoint_url(base, "api/v1/query", &[("query", SELECTOR)])
        ),
        fetcher.get_json::<PromReply>(
            SOURCE,
            endpoint_url(
                base,
                "api/v1/query",
                &[("query", ORDERS), ("time", &midnight)]
            ),
        ),
    );
    let rows = current
        .and_then(|r| rows(r, at_midnight, &label, now))
        .unwrap_or_else(|reason| {
            let value = if reason == NO_DATA {
                reason
            } else {
                format!("assente: {reason}")
            };
            vec![
                Reading::missing("nautilus_shadow ledger", &label, &value)
                    .into_row("shadow:prometheus"),
            ]
        });
    Sheet::new(ID, TITLE, rows)
}
