//! Everything that talks to LoL Esports

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

#[cfg(test)]
mod tests {
    use super::http;

    #[test]
    fn http_client_is_shared() {
        assert!(std::ptr::eq(http(), http()));
    }
}
