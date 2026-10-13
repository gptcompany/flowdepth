use std::net::SocketAddr;

use thiserror::Error;
use url::Url;

pub const DEFAULT_BIND: &str = "127.0.0.1:8790";

/// Source-specific staleness thresholds (seconds). Rektslug supplies its own per-stream SLA;
/// the constant is the fallback when the payload carries none.
pub const PROM_STALE_S: i64 = 120;
pub const ALERT_STALE_S: i64 = 300;
pub const REKTSLUG_STALE_S: i64 = 600;

/// State of one optional source URL.
#[derive(Debug, Clone)]
pub enum Endpoint {
    Unset(&'static str),
    Invalid { env: &'static str, reason: String },
    Ready(Url),
}

impl Endpoint {
    fn parse(env: &'static str, raw: Option<String>) -> Self {
        let Some(raw) = raw.map(|r| r.trim().to_owned()).filter(|r| !r.is_empty()) else {
            return Self::Unset(env);
        };
        match Url::parse(&raw) {
            Ok(url) if url.cannot_be_a_base() || !matches!(url.scheme(), "http" | "https") => {
                Self::Invalid {
                    env,
                    reason: "expected an http(s) URL".to_owned(),
                }
            }
            Ok(mut url) => {
                if !url.path().ends_with('/') {
                    let path = format!("{}/", url.path());
                    url.set_path(&path);
                }
                Self::Ready(url)
            }
            Err(e) => Self::Invalid {
                env,
                reason: e.to_string(),
            },
        }
    }

    /// Name shown in cells and logs: the URL without credentials, or the env var name.
    pub fn label(&self) -> String {
        match self {
            Self::Unset(env) | Self::Invalid { env, .. } => (*env).to_owned(),
            Self::Ready(url) => redact(url).to_string(),
        }
    }

    /// Reason this source cannot be queried, if any.
    pub fn unavailable_reason(&self) -> Option<String> {
        match self {
            Self::Unset(env) => Some(format!("{env} not set")),
            Self::Invalid { env, reason } => Some(format!("{env} invalid: {reason}")),
            Self::Ready(_) => None,
        }
    }
}

/// Strips userinfo so credentials never reach cells or logs.
pub fn redact(url: &Url) -> Url {
    let mut clean = url.clone();
    let _ = clean.set_username("");
    let _ = clean.set_password(None);
    clean
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("DASH_BIND {value:?} is not a socket address: {source}")]
    Bind {
        value: String,
        source: std::net::AddrParseError,
    },
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub prom: Endpoint,
    pub alertmanager: Endpoint,
    pub rektslug: Endpoint,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let bind_raw = get("DASH_BIND")
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BIND.to_owned());
        let bind = bind_raw
            .trim()
            .parse()
            .map_err(|source| ConfigError::Bind {
                value: bind_raw.clone(),
                source,
            })?;
        Ok(Self {
            bind,
            prom: Endpoint::parse("DASH_PROM_URL", get("DASH_PROM_URL")),
            alertmanager: Endpoint::parse("DASH_ALERTMANAGER_URL", get("DASH_ALERTMANAGER_URL")),
            rektslug: Endpoint::parse("DASH_REKTSLUG_URL", get("DASH_REKTSLUG_URL")),
        })
    }
}
