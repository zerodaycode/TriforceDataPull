use std::time::Duration;

use reqwest::Response;
use serde::Serialize;

use crate::utils::constants::lolesports;
use color_eyre::{eyre::Context, Result};

pub async fn make_get_request<T>(endpoint: &str, args: Option<&T>) -> Result<Response>
where
    T: Serialize,
{
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;

    let mut b = client
        .get(format!("{}{}", lolesports::BASE_URL, endpoint))
        .header("x-api-key", "0TvQnueqKa5mxJntVWt0w4LpLfEkrV1Ta8rQBb9Z");

    if let Some(arguments) = args {
        b = b.query(arguments);
    }

    b.send()
        .await
        .with_context(|| format!("Failed to request data from the LoLEsports API:{endpoint:?}"))
}
