use reqwest::IntoUrl;
use serde::Serialize;

use crate::utils::constants::lolesports;
use crate::data_pull::serde_models::LolesportsId;

pub async fn make_get_request<T>(endpoint: &str, args: Option<&T>) 
    -> Result<reqwest::Response, reqwest::Error>
    where T: Serialize
{
    let client = reqwest::Client::new();

    let mut b = client
        .get(format!("{}{}",lolesports::base_url, endpoint))
        .header("x-api-key", "0TvQnueqKa5mxJntVWt0w4LpLfEkrV1Ta8rQBb9Z");
    
    if let Some(arguments) = args {
        b = b.query(arguments);
    }
    
    b.send().await
}