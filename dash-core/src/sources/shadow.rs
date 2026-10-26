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
const SELECTOR: &str = r#"{__name__=~"nautilus_shadow_(orders_total|fills_total|blocked_without_id_total|realized_pnl_usd|last_order_timestamp_seconds|exporter_run_timestamp_seconds)"}"#;
const ORDERS: &str = "nautilus_shadow_orders_total";
const RUN_TS: &str = "nautilus_shadow_exporter_run_timestamp_seconds";
/// An exporter run older than this makes every ledger row stale. The exporter timer is not
/// deployed yet; 15 minutes leaves room for a 5-minute timer.
pub const EXPORTER_STALE_S: i64 = 900;
pub const NO_DATA: &str = "nessun dato dal ledger shadow";
/// Shown on the PnL until nautilus_dev F-033/F-035 are closed in its findings register.
pub const UNRELIABLE: &str = "non affidabile";

/// `status` values of `orders_total` shown as columns, with their title. The counter is "orders
/// that reached status S", one increment per status reached (B2, gbg-agents#883, 2026-10-26), so
/// statuses are never summed together. `submitted` counts simulated orders: denied orders reach
/// `initialized` but not `submitted`, so they stay apart; `blocked` orders, stopped by the audit
/// gate, reach neither. Values emitted by the exporter in nautilus_dev#566.
pub const ORDER_STATUSES: [(&str, &str); 4] = [
    ("submitted", "ordini simulati"),
    ("denied", "denied"),
    ("filled", "filled"),
    ("blocked", "bloccati"),
];

#[derive(Default)]
struct Strategy {
    /// `orders_total` per `status`, summed over `side`.
    orders: BTreeMap<String, f64>,
    fills: Option<f64>,
    blocked_without_id: Option<f64>,
    pnl: Option<f64>,
    last_order: Option<f64>,
}

pub const UNASSESSABLE: &str = "non valutabile";

/// Sample value, with any non-finite or unparsable value as NaN: it then poisons every sum it
/// enters, so a partial total can never be shown as if it were complete.
fn value_or_nan(raw: &str) -> f64 {
    finite(raw).unwrap_or(f64::NAN)
}

type ByStatus = BTreeMap<(String, String), f64>;

/// Values summed per (`strategy`, `status`), i.e. over `side`; `name` keeps only that metric,
/// `None` takes every series (aggregated replies carry no `__name__`).
fn by_status(reply: PromReply, name: Option<&str>) -> Result<ByStatus, String> {
    let mut out = BTreeMap::new();
    for s in samples(reply)? {
        if name.is_none_or(|n| label(&s, "__name__") == n) {
            let key = (
                label(&s, "strategy").to_owned(),
                label(&s, "status").to_owned(),
            );
            *out.entry(key).or_insert(0.0) += value_or_nan(&s.value.1);
        }
    }
    Ok(out)
}

/// Prometheus `query.lookback-delta` (default, read from `/api/v1/status/flags` on 2026-10-26).
/// The 00:00Z baseline is the last sample up to this long before midnight.
pub const LOOKBACK_S: i64 = 300;

/// PromQL counting `orders_total` resets per strategy and status since the 00:00Z baseline.
/// The window starts a lookback before midnight, so a reset between the baseline sample and the
/// first sample after midnight is counted; a reset just before the baseline sample can only add
/// a spurious «non valutabile», never a false `ok`.
pub fn resets_query(now: DateTime<Utc>) -> String {
    let window = now.timestamp() - midnight_utc(now) + LOOKBACK_S;
    format!("sum by (strategy, status) (resets({ORDERS}[{window}s]))")
}

/// `now_reply` is the current ledger state; `midnight` holds `orders_total` at 00:00Z of `now`'s
/// day, from which each status's "today" is the difference, valid only if `resets` (from
/// [`resets_query`]) reports none for that status since midnight; a failed query blanks them.
pub fn rows(
    now_reply: PromReply,
    midnight: Result<PromReply, String>,
    resets: Result<PromReply, String>,
    source: &str,
    now: DateTime<Utc>,
) -> Result<Vec<Row>, String> {
    let mut strategies: BTreeMap<String, Strategy> = BTreeMap::new();
    let mut run = None;
    for s in samples(now_reply)? {
        let name = label(&s, "__name__");
        if name == RUN_TS {
            run = finite(&s.value.1);
            continue;
        }
        // Only the exporter's names open a strategy: other producers share the prefix.
        let slot: fn(&mut Strategy) -> &mut Option<f64> = match name {
            ORDERS => {
                let st = strategies
                    .entry(label(&s, "strategy").to_owned())
                    .or_default();
                let n = st
                    .orders
                    .entry(label(&s, "status").to_owned())
                    .or_insert(0.0);
                *n += value_or_nan(&s.value.1);
                continue;
            }
            "nautilus_shadow_fills_total" => |st| &mut st.fills,
            "nautilus_shadow_blocked_without_id_total" => |st| &mut st.blocked_without_id,
            "nautilus_shadow_realized_pnl_usd" => |st| &mut st.pnl,
            "nautilus_shadow_last_order_timestamp_seconds" => |st| &mut st.last_order,
            _ => continue,
        };
        let v = value_or_nan(&s.value.1);
        let st = strategies
            .entry(label(&s, "strategy").to_owned())
            .or_default();
        let acc = slot(st);
        *acc = Some(acc.unwrap_or(0.0) + v);
    }
    if run.is_none() && strategies.is_empty() {
        return Err(NO_DATA.to_owned());
    }

    let (base, exporter) = run_freshness("esportatore ledger".into(), run, now, EXPORTER_STALE_S);
    let at_midnight = midnight.and_then(|r| by_status(r, Some(ORDERS)));
    let resets = resets.and_then(|r| by_status(r, None));
    let mut out = Vec::new();
    for (name, st) in &strategies {
        let comp = |k: &str| format!("{name} · {k}");
        let push = |out: &mut Vec<(String, Reading)>, k: &str, r: Reading| {
            out.push((format!("shadow:{name}:{k}"), r));
        };
        // Never `ok`: the value exists but cannot be trusted.
        let unassessable = |k: &str, why: &str| Reading {
            status: UNASSESSABLE.into(),
            severity: Severity::Warn,
            ..Reading::missing(&comp(k), "", &format!("{UNASSESSABLE}: {why}"))
        };
        let count = |k: &str, v: Option<f64>| match v {
            Some(v) if !v.is_finite() => unassessable(k, "serie non numerica"),
            Some(v) => derived(&exporter, base, comp(k), format!("{v:.0}")),
            None => Reading::missing(&comp(k), "", "assente"),
        };
        push(&mut out, "fill", count("fill", st.fills));
        let k = "bloccati senza id";
        push(&mut out, k, count(k, st.blocked_without_id));

        for (status, title) in ORDER_STATUSES {
            let total = st.orders.get(status).copied();
            let k = format!("{title} totali");
            push(&mut out, &k, count(&k, total));

            let k = format!("{title} oggi");
            let key = (name.clone(), status.to_owned());
            let today = match (total, &at_midnight, &resets) {
                (None, ..) => Reading::missing(&comp(&k), "", "assente"),
                (Some(_), Err(reason), _) | (Some(_), _, Err(reason)) => {
                    Reading::missing(&comp(&k), "", &format!("assente: {reason}"))
                }
                (Some(total), Ok(m), Ok(r)) => match (m.get(&key), r.get(&key)) {
                    (None, _) => {
                        Reading::missing(&comp(&k), "", "assente: nessun campione alle 00:00Z")
                    }
                    (_, None) => {
                        Reading::missing(&comp(&k), "", "assente: nessun campione dalle 00:00Z")
                    }
                    (Some(start), Some(n)) if !(total - start).is_finite() || !n.is_finite() => {
                        unassessable(&k, "serie non numerica")
                    }
                    // After a reset since midnight "now minus midnight" means nothing.
                    (Some(start), Some(n)) if *n > 0.0 || total < *start => {
                        unassessable(&k, "contatore azzerato")
                    }
                    (Some(start), _) => count(&k, Some(total - start)),
                },
            };
            push(&mut out, &k, today);
        }

        let k = "ultimo ordine";
        let last = match st
            .last_order
            .map(|v| (v, DateTime::from_timestamp(v as i64, 0)))
        {
            Some((v, Some(t))) if v.is_finite() => Reading {
                ts: Some(t),
                age_s: Some(age_secs(t, now)),
                ..derived(
                    &exporter,
                    base,
                    comp(k),
                    format!("{} min fa", age_secs(t, now) / 60),
                )
            },
            Some(_) => unassessable(k, "serie non numerica"),
            None => Reading::missing(&comp(k), "", "assente"),
        };
        push(&mut out, k, last);

        // Simulated PnL is never `ok` while the accounting findings are open.
        let k = "PnL realizzato simulato";
        let pnl = match st.pnl {
            Some(v) if !v.is_finite() => unassessable(k, "serie non numerica"),
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
    let resets = resets_query(now);
    let (current, at_midnight, resets) = tokio::join!(
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
        fetcher.get_json::<PromReply>(
            SOURCE,
            endpoint_url(base, "api/v1/query", &[("query", &resets)])
        ),
    );
    let rows = current
        .and_then(|r| rows(r, at_midnight, resets, &label, now))
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
