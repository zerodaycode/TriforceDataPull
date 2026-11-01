//! Everything that talks to LoL Esports

pub mod adapters;
pub mod feed;
pub mod gql;
pub mod legacy;

use std::{sync::OnceLock, time::Duration};

/// One HTTP client for the whole app, so connections are reused between calls
pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            // HTTP/2 pings, so a dead connection is dropped instead of reused by every loop
            .http2_keep_alive_interval(Duration::from_secs(10))
            .http2_keep_alive_timeout(Duration::from_secs(5))
            .http2_keep_alive_while_idle(true)
            // longer than the 2 min live loop, so the connection is kept between ticks
            .pool_idle_timeout(Duration::from_secs(300))
            .build()
            .expect("failed to build the HTTP client")
    })
}

/// A list sent as `null` (or missing, with `#[serde(default)]`): empty
pub(crate) fn null_as_empty<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    use serde::Deserialize;
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// Biggest body we read, decompressed. The biggest real answer is ~1.2 MB
pub const MAX_BODY: usize = 32 * 1024 * 1024;

#[derive(Debug)]
pub enum BodyError {
    Network(reqwest::Error),
    /// over this many bytes
    TooBig(usize),
    NotUtf8,
}

impl std::fmt::Display for BodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(e) => write!(f, "network error: {e}"),
            Self::TooBig(max) => write!(f, "answer over {max} bytes"),
            Self::NotUtf8 => write!(f, "answer is not UTF-8"),
        }
    }
}

impl std::error::Error for BodyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            // Display already has the reqwest error: start the chain one level down
            Self::Network(e) => std::error::Error::source(e),
            _ => None,
        }
    }
}

/// The body as text, up to `MAX_BODY`: a small compressed answer can expand a lot
pub async fn read_text(response: reqwest::Response) -> Result<String, BodyError> {
    read_text_max(response, MAX_BODY).await
}

/// The body as text, up to `max` bytes
pub async fn read_text_max(
    mut response: reqwest::Response,
    max: usize,
) -> Result<String, BodyError> {
    let mut buf = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| BodyError::Network(e.without_url()))?
    {
        if buf.len() + chunk.len() > max {
            return Err(BodyError::TooBig(max));
        }
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf).map_err(|_| BodyError::NotUtf8)
}

/// Where a data set comes from while we move to the new API
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// esports-api.lolesports.com
    Old,
    /// lolesports.com GraphQL
    #[default]
    New,
}

impl Source {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "old" => Some(Self::Old),
            "new" => Some(Self::New),
            _ => None,
        }
    }

    /// Reads `var` (`old` or `new`), not set means `new`
    pub fn from_env(var: &str) -> color_eyre::Result<Self> {
        match crate::env::var(var) {
            Err(std::env::VarError::NotPresent) => Self::from_value(var, None),
            Err(e) => Err(color_eyre::eyre::eyre!("{var}: {e}")),
            Ok(v) => Self::from_value(var, Some(&v)),
        }
    }

    fn from_value(var: &str, value: Option<&str>) -> color_eyre::Result<Self> {
        match value {
            None => Ok(Self::default()),
            Some(v) => Self::parse(v)
                .ok_or_else(|| color_eyre::eyre::eyre!("{var} must be old or new, got {v:?}")),
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Old => "old",
            Self::New => "new",
        })
    }
}

pub fn log(msg: impl std::fmt::Display) {
    println!(
        "{} - {msg}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%f")
    );
}

#[cfg(test)]
mod tests {
    use super::{Source, http};

    #[test]
    fn http_client_is_shared() {
        assert!(std::ptr::eq(http(), http()));
    }

    #[test]
    fn source_parse() {
        assert_eq!(Source::parse("old"), Some(Source::Old));
        assert_eq!(Source::parse("new"), Some(Source::New));
        assert_eq!(Source::parse(" NEW "), Some(Source::New));
        assert_eq!(Source::parse("graphql"), None);
        assert_eq!(Source::default(), Source::New);
    }

    #[test]
    fn source_from_value() {
        let v = "TRIFORCE_SOURCE_TEAMS";
        assert_eq!(Source::from_value(v, None).unwrap(), Source::New);
        assert_eq!(Source::from_value(v, Some("old")).unwrap(), Source::Old);
        let e = Source::from_value(v, Some("nwe")).unwrap_err().to_string();
        assert_eq!(e, r#"TRIFORCE_SOURCE_TEAMS must be old or new, got "nwe""#);
    }
}
