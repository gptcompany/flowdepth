use dash_core::config::{Config, DEFAULT_BIND, Endpoint};

fn cfg(pairs: &[(&str, &str)]) -> Result<Config, dash_core::config::ConfigError> {
    Config::from_lookup(|k| {
        pairs
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| (*v).to_owned())
    })
}

#[test]
fn defaults_bind_and_all_sources_unset() {
    let c = cfg(&[]).unwrap();
    assert_eq!(c.bind.to_string(), DEFAULT_BIND);
    assert!(matches!(c.prom, Endpoint::Unset("DASH_PROM_URL")));
    assert!(matches!(c.rektslug, Endpoint::Unset(_)));
}

#[test]
fn bind_is_configurable_and_validated() {
    assert_eq!(
        cfg(&[("DASH_BIND", "0.0.0.0:9000")]).unwrap().bind.port(),
        9000
    );
    assert!(cfg(&[("DASH_BIND", "nope")]).is_err());
}

#[test]
fn credentials_never_reach_the_source_label() {
    let c = cfg(&[(
        "DASH_PROM_URL",
        "https://user:s3cret@prom.example:9090/base",
    )])
    .unwrap();
    let label = c.prom.label();
    assert_eq!(label, "https://prom.example:9090/base/");
    assert!(!label.contains("s3cret") && !label.contains("user"));
}

#[test]
fn invalid_url_is_reported_not_fatal() {
    let c = cfg(&[
        ("DASH_PROM_URL", "not a url"),
        ("DASH_REKTSLUG_URL", "ftp://x"),
    ])
    .unwrap();
    assert!(
        c.prom
            .unavailable_reason()
            .unwrap()
            .starts_with("DASH_PROM_URL invalid")
    );
    assert!(c.rektslug.unavailable_reason().unwrap().contains("http(s)"));
}
