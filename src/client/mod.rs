//! Everything that talks to LoL Esports

pub mod adapters;
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

/// Where a data set comes from while we move to the new API
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// esports-api.lolesports.com
    #[default]
    Old,
    /// lolesports.com GraphQL
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

    /// Reads `var` (`old` or `new`), not set means `old`
    pub fn from_env(var: &str) -> color_eyre::Result<Self> {
        match std::env::var(var) {
            Err(std::env::VarError::NotPresent) => Ok(Self::Old),
            Err(e) => Err(color_eyre::eyre::eyre!("{var}: {e}")),
            Ok(v) => Self::parse(&v)
                .ok_or_else(|| color_eyre::eyre::eyre!("{var} must be old or new, got {v:?}")),
        }
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
        assert_eq!(Source::default(), Source::Old);
    }
}
