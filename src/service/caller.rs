use crate::utils::constants::lolesports;
use crate::data_pull::serde_models::LolesportsId;

pub async fn make_get_request(endpoint: &str, args: &[(&str, LolesportsId)]) -> Result<reqwest::Response, reqwest::Error> {
    let client = reqwest::Client::new();

    client
        .get(format!("{}{}",lolesports::base_url, endpoint))
        .header("x-api-key", "0TvQnueqKa5mxJntVWt0w4LpLfEkrV1Ta8rQBb9Z")
        .query(args)
        .send()
        .await
}