use chrono::Local;
use reqwest::{Error, Response};
use serde::Serialize;
use std::fmt::Debug;
use std::time::Duration;

use crate::utils::constants::lolesports;
use color_eyre::{eyre::Context, Result};
use tokio::time::sleep;

pub async fn make_get_request<T>(endpoint: &str, args: Option<&T>) -> Result<Response>
where
    T: Serialize + Debug,
{
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;

    let mut attempts = 2;
    let retry_duration = Duration::from_secs(5);

    loop {
        let mut b = client
            .get(format!("{}{}", lolesports::BASE_URL, endpoint))
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
