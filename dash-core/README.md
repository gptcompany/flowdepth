# dash-core

Headless Rust service behind the flowdepth dashboard. `GET /v1/sheets` aggregates three read-only
sources into one JSON document (contract v1, canonical example: `fixtures/sheets.json`).
A failing or unconfigured source yields `missing` cells carrying the reason; the response is
never a 500.

## Run

```sh
DASH_PROM_URL=http://127.0.0.1:9090 \
DASH_ALERTMANAGER_URL=http://127.0.0.1:9093 \
DASH_REKTSLUG_URL=http://127.0.0.1:8002 \
cargo run -p dash-core
curl -s http://127.0.0.1:8790/v1/sheets | jq .
```

| env | default | meaning |
|---|---|---|
| `DASH_BIND` | `127.0.0.1:8790` | listen address |
| `DASH_PROM_URL` | unset → `infra` cells `missing` | Prometheus base URL |
| `DASH_ALERTMANAGER_URL` | unset → `alerts` cells `missing` | Alertmanager base URL |
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
| `alerts` | `/api/v2/alerts?active=true&silenced=false&inhibited=false` | one per active alert |
| `rektslug` | `/health`, `/ops/summary` | `/health`, the ops envelope, and one per `<table>:<symbol>` stream in `details.data_freshness` with its `data_as_of` |

Severity: `ok|warn|crit` from the datum; `stale` when its timestamp is older than the source
threshold (Prometheus 120 s, Alertmanager `updatedAt` 300 s, rektslug the stream's own `sla_secs`,
fallback 600 s); `missing` when the source or datum is absent. Alertmanager `severity` label:
`critical|crit|page|error|fatal` → `crit`, `info|none|ok` → `ok`, anything else or absent → `warn`.

## Where the real endpoints are

Found in `gptcompany/monitoring-stack` (README, `prometheus/prometheus.yml`) and
`gptcompany/rektslug` (`docker-compose.yml`, `src/liquidationheatmap/api`):

| source | URL on the workstation | evidence |
|---|---|---|
| Prometheus | `http://127.0.0.1:9090` | monitoring-stack README: `prometheus (:9090)`, `curl localhost:9090/api/v1/targets` |
| Alertmanager | `http://127.0.0.1:9093` | README `alertmanager (:9093)`; `prometheus.yml` alerting target `localhost:9093` |
| rektslug | `http://127.0.0.1:8002` | compose `${HEATMAP_PORT:-8002}:8002`, healthcheck `http://127.0.0.1:8002/health` |

These are the workstation-local bindings; when dash-core runs on another host, reach them through
the existing tunnel/VPN, not by exposing the ports. The `hl_*` series (e.g. `hl_blocks_behind`) come
from the Hyperliquid exporters scraped by the `hyperliquid-exporter` and `vps-hyperliquid-exporter`
jobs.

## Known gaps (TODO)

- Prometheus instant queries stamp samples with the evaluation time, so Prometheus staleness only
  catches clock skew or a caching proxy. A true sample age needs `timestamp(<selector>)`.
- `hl_*` rows are `ok` whenever the value is finite; no per-metric thresholds are defined yet.
- No response caching: each `GET /v1/sheets` fans out to all sources (5 s timeout per request).

## Tests

`cargo test -p dash-core` — recorded responses in `fixtures/recorded/`, HTTP mocks via wiremock,
and the serialized contract compared with `fixtures/sheets.json`.
