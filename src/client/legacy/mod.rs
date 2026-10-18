//! Old LoL Esports API (esports-api.lolesports.com/persisted/gw)

pub mod endpoints;
pub mod models;

use chrono::Local;
use color_eyre::{Result, eyre::Context};
use reqwest::Response;
use serde::Serialize;
use std::fmt::Debug;
use std::time::Duration;
use tokio::time::sleep;

/// Body of a GET without args, an error on a non-2xx status
pub async fn get_text(endpoint: &str) -> Result<String> {
    Ok(get::<()>(endpoint, None)
        .await?
        .error_for_status()?
        .text()
        .await?)
}

pub async fn get<T>(endpoint: &str, args: Option<&T>) -> Result<Response>
where
    T: Serialize + Debug,
{
    let client = super::http();

    let mut attempts = 2;
    let retry_duration = Duration::from_secs(5);

    loop {
        let mut b = client
            .get(format!("{}{}", endpoints::BASE_URL, endpoint))
            .header("x-api-key", "0TvQnueqKa5mxJntVWt0w4LpLfEkrV1Ta8rQBb9Z");

        if let Some(arguments) = args {
            b = b.query(arguments);
        }

        let result = b.send().await;

        match result {
            Ok(response) => return Ok(response),
            Err(e) => {
                if e.is_timeout() && attempts > 0 {
                    attempts -= 1;
                    println!(
                        "{} - Request to {} with args {:?} timed out ",
                        Local::now().format("%Y-%m-%d %H:%M:%S.%f"),
                        &endpoint,
                        args
                    );

                    sleep(retry_duration).await;
                } else {
                    return Err(e)
                        .with_context(|| format!("Failed to request data from the LoLEsports API:{endpoint:?} with args -> {args:?}"));
                }
            }
        }
    }
}
