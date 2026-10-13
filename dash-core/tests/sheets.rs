//! Collector behaviour against mocked HTTP sources, and the serialized v1 contract.

mod common;

use std::sync::Arc;

use common::{config, fixture, mount_all_status, mount_prom, mount_recorded, now};
use dash_core::model::{COLUMNS, Severity, Sheet, SheetsResponse};
use dash_core::{Collector, router};
use wiremock::MockServer;

fn sheet<'a>(r: &'a SheetsResponse, id: &str) -> &'a Sheet {
    r.sheets
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("sheet {id} missing"))
}

async fn collect(cfg: dash_core::config::Config) -> SheetsResponse {
    Collector::new(cfg).unwrap().collect(now()).await
}

fn assert_all_missing(s: &Sheet) {
    assert!(!s.rows.is_empty(), "sheet {} has no rows", s.id);
    for row in &s.rows {
        assert_eq!(row.cells.len(), COLUMNS.len());
        assert!(
            row.cells
                .iter()
                .all(|c| c.severity == Severity::Missing && c.ts.is_none()),
            "row {} not fully missing: {row:#?}",
            row.id
        );
    }
}

#[tokio::test]
async fn unset_env_yields_missing_cells_naming_the_variable() {
    let r = collect(config(None, None)).await;
    assert_eq!(
        r.sheets.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["infra", "alerts", "rektslug"]
    );
    for (id, var) in [
        ("infra", "DASH_PROM_URL"),
        ("alerts", "DASH_PROM_URL"),
        ("rektslug", "DASH_REKTSLUG_URL"),
    ] {
        let s = sheet(&r, id);
        assert_all_missing(s);
        assert_eq!(s.rows[0].cells[2].value, format!("{var} not set"));
        assert_eq!(s.rows[0].cells[3].value, var);
    }
}

#[tokio::test]
async fn source_returning_5xx_yields_missing_with_reason() {
    let (prom, rekt) = (MockServer::start().await, MockServer::start().await);
    for s in [&prom, &rekt] {
        mount_all_status(s, 503).await;
    }
    let r = collect(config(Some(&prom.uri()), Some(&rekt.uri()))).await;
    for id in ["infra", "alerts", "rektslug"] {
        let s = sheet(&r, id);
        assert_all_missing(s);
        assert!(
            s.rows.iter().all(|row| row.cells[2].value == "http 503"),
            "{s:#?}"
        );
    }
}

#[tokio::test]
async fn unreachable_source_yields_missing_with_reason() {
    // Bind then drop: the port is free, so the connection is refused.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let url = format!("http://127.0.0.1:{port}");
    let r = collect(config(Some(&url), Some(&url))).await;
    for id in ["infra", "alerts", "rektslug"] {
        let s = sheet(&r, id);
        assert_all_missing(s);
        assert!(
            s.rows[0].cells[2].value.starts_with("request failed"),
            "{s:#?}"
        );
    }
}

#[tokio::test]
async fn malformed_body_yields_missing_not_error() {
    let prom = MockServer::start().await;
    common::mount_alerts(&prom, "<html>bad gateway</html>".to_owned()).await;
    let r = collect(config(Some(&prom.uri()), None)).await;
    let s = sheet(&r, "alerts");
    assert_all_missing(s);
    assert!(s.rows[0].cells[2].value.starts_with("invalid response"));
}

#[tokio::test]
async fn alerts_reply_with_wrong_shape_or_error_status_is_missing() {
    for (body, reason) in [
        (
            r#"{"status":"success","data":{"alerts":"nope"}}"#,
            "invalid response",
        ),
        (r#"{"status":"success"}"#, "success reply without data"),
        (
            r#"{"status":"error","errorType":"unavailable","error":"rules not loaded"}"#,
            "rules not loaded",
        ),
    ] {
        let prom = MockServer::start().await;
        common::mount_alerts(&prom, body.to_owned()).await;
        let r = collect(config(Some(&prom.uri()), None)).await;
        let s = sheet(&r, "alerts");
        assert_all_missing(s);
        assert!(s.rows[0].cells[2].value.starts_with(reason), "{s:#?}");
    }
}

#[tokio::test]
async fn alerts_are_served_from_prometheus_over_http() {
    let prom = MockServer::start().await;
    common::mount_alerts(&prom, fixture("recorded/prometheus_alerts.json")).await;
    let r = collect(config(Some(&prom.uri()), None)).await;
    let s = sheet(&r, "alerts");
    assert_eq!(s.rows.len(), 4);
    let sev = |id: &str| {
        s.rows
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("row {id} missing in {s:#?}"))
            .cells[0]
            .severity
    };
    assert_eq!(
        sev("alert:alertname=HLNodeBehind,instance=localhost:9101,severity=critical"),
        Severity::Crit
    );
    assert_eq!(
        sev("alert:alertname=TextfileStale,severity=critical"),
        Severity::Warn
    );
    // Reads only /api/v1/alerts, never an Alertmanager path.
    let paths: Vec<String> = prom
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|req| req.url.path().to_owned())
        .collect();
    assert!(paths.iter().any(|p| p == "/api/v1/alerts"), "{paths:?}");
    assert!(paths.iter().all(|p| p.starts_with("/api/v1/")), "{paths:?}");
}

#[tokio::test]
async fn alerts_failure_keeps_infra_rows() {
    let prom = MockServer::start().await;
    mount_prom(
        &prom,
        fixture("recorded/prom_up.json"),
        fixture("recorded/prom_hl.json"),
    )
    .await;
    // /api/v1/alerts is unmocked → wiremock 404.
    let r = collect(config(Some(&prom.uri()), None)).await;
    assert_all_missing(sheet(&r, "alerts"));
    assert_eq!(sheet(&r, "alerts").rows[0].cells[2].value, "http 404");
    assert!(
        sheet(&r, "infra")
            .rows
            .iter()
            .any(|row| row.cells[0].severity == Severity::Ok)
    );
}

#[tokio::test]
async fn one_source_down_does_not_affect_the_others() {
    let (prom, rekt) = (MockServer::start().await, MockServer::start().await);
    mount_recorded(&prom, &rekt).await;
    let r = collect(config(Some(&prom.uri()), Some("http://127.0.0.1:1"))).await;
    assert_all_missing(sheet(&r, "rektslug"));
    assert!(
        sheet(&r, "infra")
            .rows
            .iter()
            .any(|row| row.cells[0].severity == Severity::Ok)
    );
    assert!(
        sheet(&r, "alerts")
            .rows
            .iter()
            .any(|row| row.cells[0].severity == Severity::Crit)
    );
}

#[tokio::test]
async fn hl_query_failure_keeps_up_rows() {
    let prom = MockServer::start().await;
    // Only `up` is mocked; the hl_* query falls through to wiremock's 404.
    wiremock::Mock::given(wiremock::matchers::query_param("query", "up"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_raw(fixture("recorded/prom_up.json"), "application/json"),
        )
        .mount(&prom)
        .await;
    let r = collect(config(Some(&prom.uri()), None)).await;
    let infra = sheet(&r, "infra");
    assert_eq!(infra.rows.len(), 4);
    let hl = infra.rows.iter().find(|row| row.id == "infra:hl").unwrap();
    assert_eq!(hl.cells[0].severity, Severity::Missing);
    assert_eq!(hl.cells[2].value, "http 404");
}

#[tokio::test]
async fn stale_sample_is_flagged_stale() {
    let prom = MockServer::start().await;
    let old = r#"{"status":"success","data":{"resultType":"vector","result":[
        {"metric":{"job":"j","instance":"i"},"value":[1791892000,"1"]}]}}"#;
    mount_prom(
        &prom,
        old.to_owned(),
        r#"{"status":"success","data":{"resultType":"vector","result":[]}}"#.to_owned(),
    )
    .await;
    let r = collect(config(Some(&prom.uri()), None)).await;
    let row = &sheet(&r, "infra").rows[0];
    assert!(row.cells.iter().all(|c| c.severity == Severity::Stale));
    assert_eq!(row.cells[4].value, "800");
}

#[tokio::test]
async fn serialized_contract_equals_fixture() {
    let (prom, rekt) = (MockServer::start().await, MockServer::start().await);
    mount_recorded(&prom, &rekt).await;
    let r = collect(config(Some(&prom.uri()), Some(&rekt.uri()))).await;

    // Mock ports are random; map them to the real default ports so the fixture is stable.
    let mut text = serde_json::to_string(&r).unwrap();
    for (mock, real) in [(&prom, "9090"), (&rekt, "8002")] {
        text = text.replace(
            &format!("{}/", mock.uri()),
            &format!("http://127.0.0.1:{real}/"),
        );
    }
    // Typed round trip keeps contract field order in the printed diagnostic.
    let typed: SheetsResponse = serde_json::from_str(&text).unwrap();
    let actual = serde_json::to_value(&typed).unwrap();
    let expected: serde_json::Value = serde_json::from_str(&fixture("sheets.json")).unwrap();
    assert_eq!(
        actual,
        expected,
        "contract drifted from fixtures/sheets.json; actual:\n{}",
        serde_json::to_string_pretty(&typed).unwrap()
    );
}

#[test]
fn fixture_is_a_valid_contract_document() {
    let raw = fixture("sheets.json");
    let parsed: SheetsResponse = serde_json::from_str(&raw).unwrap();
    // Typed round trip must not add, drop or rename a field.
    assert_eq!(
        serde_json::to_value(&parsed).unwrap(),
        serde_json::from_str::<serde_json::Value>(&raw).unwrap()
    );
    for s in &parsed.sheets {
        assert_eq!(s.columns, COLUMNS);
        for row in &s.rows {
            assert_eq!(row.cells.len(), s.columns.len(), "row {}", row.id);
        }
    }
    let seen: std::collections::HashSet<_> = parsed
        .sheets
        .iter()
        .flat_map(|s| &s.rows)
        .flat_map(|r| &r.cells)
        .map(|c| c.severity)
        .collect();
    assert_eq!(
        seen.len(),
        5,
        "fixture should exercise every severity, saw {seen:?}"
    );
}

#[tokio::test]
async fn http_endpoint_serves_200_even_when_every_source_is_down() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let collector = Arc::new(Collector::new(config(None, None)).unwrap());
    tokio::spawn(async move { axum::serve(listener, router(collector)).await.unwrap() });

    let resp = reqwest::get(format!("http://{addr}/v1/sheets"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: SheetsResponse = resp.json().await.unwrap();
    assert_eq!(body.sheets.len(), 3);
    body.sheets.iter().for_each(assert_all_missing);
}
