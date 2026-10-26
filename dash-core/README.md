# dash-core

Rust service behind the flowdepth dashboard (JSON API plus a static operator page at `/`). `GET /v1/sheets` aggregates two read-only
sources into one JSON document (contract v1, canonical example: `fixtures/sheets.json` at the workspace root, the single copy shared with the frontend).
A failing or unconfigured source yields `missing` cells carrying the reason; the response is
never a 500.

## Run

```sh
DASH_PROM_URL=http://127.0.0.1:9090 \
DASH_REKTSLUG_URL=http://127.0.0.1:8002 \
cargo run -p dash-core
curl -s http://127.0.0.1:8790/v1/sheets | jq .
```

| env | default | meaning |
|---|---|---|
| `DASH_BIND` | `127.0.0.1:8790` | listen address |
| `DASH_PROM_URL` | unset → `infra`, `alerts`, `completeness`, `shadow` cells `missing` | Prometheus base URL (read-only: `/api/v1/query`, `/api/v1/alerts`) |
| `DASH_REKTSLUG_URL` | unset → `rektslug` cells `missing` | rektslug base URL |

`RUST_LOG` filters logs (default `info`). Logs are JSON, one line per fetch with
`source`, `endpoint`, `duration_ms`, `outcome` (`ok`|`error`) and `error`; credentials in URLs are
stripped before logging and before appearing in cells.

## Sheets

All sheets share the columns `component, status, value, source, age_s`; every cell of a row carries
the same severity.

| sheet | requests (GET only) | rows |
|---|---|---|
| `infra` | `/api/v1/query?query=up`, `/api/v1/query?query={__name__=~"hl_.*"}` | one per `up` series (`job/instance`), one per `hl_*` series |
| `alerts` | Prometheus `/api/v1/alerts` | one per `pending` or `firing` alert, keyed by its label set |
| `rektslug` | `/health`, `/ops/summary` | `/health`, the ops envelope, and one per `<table>:<symbol>` stream in `details.data_freshness` with its `data_as_of` |
| `completeness` | `/api/v1/query?query={__name__=~"hl_block_continuity_(…)\|hl_core_block_height\|hl_core_latest_block_time"}` | per HL node (`host`, else `instance`): block height, aged by `hl_core_latest_block_time` (unix seconds; latest block older than 15 min → `stale` «vecchio di N minuti», absent → `warn`, since a stuck node keeps exporting a fresh-looking height); detector run age in minutes, last verified block, gap blocks total, last gap start/end. No `hl_block_continuity_*` series for a node → one row «rilevatore non attivo» |
| `shadow` | `/api/v1/query` on the six ledger-exporter names (exact match), plus `nautilus_shadow_orders_total` at `time=<00:00Z today>` and `sum by (strategy, status) (resets(nautilus_shadow_orders_total[<s since 00:00Z + 300>s]))` (window widened by the 5m lookback so it covers the baseline sample) | exporter run age; per strategy: for each status in `ORDER_STATUSES` (`submitted` → «ordini simulati», `denied`, `filled`, `blocked` → «bloccati»; `orders_total` counts orders that reached a status, once per status, so statuses are never summed, only `side` is) the total and today (now minus 00:00Z; «non valutabile: contatore azzerato» when Prometheus counts a reset since 00:00Z, since the difference would be meaningless), fills, «bloccati senza id» (`nautilus_shadow_blocked_without_id_total`), last order age, realized PnL labelled «non affidabile» (nautilus_dev F-033/F-035). No ledger series → one row «nessun dato dal ledger shadow». `nautilus_shadow_health_*`/`_hl_*`/`_stream_*` belong to another producer and are never read |

`GET /` serves `static/index.html` (embedded with `include_str!`, no external assets): it polls
`/v1/sheets` every 30 s, shows each sheet as a table with a severity column in Italian
(`missing` → «assente», `stale` → «vecchio») and, per sheet, the latest timestamp of each source.

Freshness of the new sheets: a detector run or exporter run older than 15 minutes turns every
dependent row `stale` («vecchio di N minuti»); an absent run timestamp leaves the values visible
but `warn` («età … ignota»), never `ok`. A non-finite sample (`NaN`, `±Inf`) in any series feeding a
shadow cell, baseline included, makes that cell `warn` «non valutabile», never a partial sum. A failing Prometheus yields one `missing` row whose value
starts with «assente:».

Severity: `ok|warn|crit` from the datum; `stale` when its timestamp is older than the source
threshold (Prometheus samples 120 s, rektslug the stream's own `sla_secs`, fallback 600 s);
`missing` when the source or datum is absent. Alerts have no staleness rule: Prometheus exposes
no per-alert update time, and the cell `ts`/`age_s` is `activeAt`, so a long-firing alert reads
as firing, never `stale`.

The `value` cell of an alert is its `summary` (else `description`) annotation followed by the
expression value as `(value=<v>)`; with no annotation it is `value=<v>`, and the alert name only
when Prometheus sent neither.

Alert mapping (`state` + `severity` label, official `GET /api/v1/alerts` schema):

| state | `severity` label | status | severity |
|---|---|---|---|
| `firing` | `critical\|crit\|page\|error\|fatal` | the label | `crit` |
| `firing` | `warning` or any other value | the label | `warn` |
| `firing` | `info\|none\|ok` | the label | `ok` |
| `firing` | absent | `unknown` | `warn` |
| `pending` | any | `pending` | `warn` |

`pending` means the rule's `for:` duration has not elapsed: shown as `pending`/`warn` even when
labelled critical; it becomes `crit` once Prometheus reports it `firing`. An unreachable
Prometheus, a non-2xx status, `"status":"error"` or a malformed body yields one `missing` row
(`alerts:prometheus`) with the reason; the response is still 200.

## Where the real endpoints are

Found in `gptcompany/monitoring-stack` (README, `prometheus/prometheus.yml`) and
`gptcompany/rektslug` (`docker-compose.yml`, `src/liquidationheatmap/api`):

| source | URL on the workstation | evidence |
|---|---|---|
| Prometheus | `http://127.0.0.1:9090` | monitoring-stack README: `prometheus (:9090)`, `curl localhost:9090/api/v1/targets` |
| rektslug | `http://127.0.0.1:8002` | compose `${HEATMAP_PORT:-8002}:8002`, healthcheck `http://127.0.0.1:8002/health` |

Alerts come from Prometheus itself: Alertmanager is not exposed (monitoring-stack ms#255).
These are the workstation-local bindings; when dash-core runs on another host, reach them through
the existing tunnel/VPN, not by exposing the ports. The `hl_*` series (e.g. `hl_blocks_behind`) come
from the Hyperliquid exporters scraped by the `hyperliquid-exporter` and `vps-hyperliquid-exporter`
jobs.

## Real run

`docs/e2e/alerts-real-2026-10-14.json`: `GET /v1/sheets` against the live Prometheus
(monitoring-stack#257, read-only NodePort over WireGuard; URL only via `DASH_PROM_URL`).
196 alerts (175 firing/warning, 17 firing/critical, 3 firing/none|info, 1 pending/warning) map to
196 rows, each checked against the raw `/api/v1/alerts` reply. The Prometheus URL is replaced by
`http://<prometheus>/`.

## Known gaps (TODO)

- Prometheus instant queries stamp samples with the evaluation time, so Prometheus staleness only
  catches clock skew or a caching proxy. A true sample age needs `timestamp(<selector>)`.
- `hl_*` rows are `ok` whenever the value is finite; no per-metric thresholds are defined yet.
- No response caching: each `GET /v1/sheets` fans out to all sources (5 s timeout per request).

## Tests

`cargo test -p dash-core` — recorded responses in `fixtures/recorded/`, HTTP mocks via wiremock,
and the serialized contract compared with `fixtures/sheets.json`.
