//! lolesports.com GraphQL API. Only persisted queries: we send the hash of a site query, never our own query text

pub mod error;
pub mod manifest;
pub mod ops;

pub use error::GqlError;

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::client::log;

pub const GQL_URL: &str = "https://lolesports.com/api/gql";
pub const SITE_URL: &str = "https://lolesports.com";
pub const CLIENT_NAME: &str = "Esports Web";

pub fn user_agent() -> String {
    format!("TriforceDataPull/{}", env!("CARGO_PKG_VERSION"))
}

/// A site query we can call. The hash comes from the manifest by `NAME`
pub trait Operation {
    const NAME: &'static str;
    type Vars: Serialize;
    type Data: DeserializeOwned;
}

#[derive(Debug)]
pub struct Answer<T> {
    pub data: T,
    /// When the CDN made the answer (`Date - Age`), not when we got it
    pub made_at: DateTime<Utc>,
}

pub fn query_params<Op: Operation>(
    hash: &str,
    vars: &Op::Vars,
) -> Result<[(&'static str, String); 3], GqlError> {
    let extensions = serde_json::json!({"persistedQuery": {"version": 1, "sha256Hash": hash}});
    Ok([
        ("operationName", Op::NAME.to_string()),
        (
            "variables",
            serde_json::to_string(vars).map_err(GqlError::Parse)?,
        ),
        ("extensions", extensions.to_string()),
    ])
}

/// Netlify sends `Date` = now, so the answer was made at `Date - Age`.
/// No usable `Date` → the time we got it
pub fn made_at(date: Option<&str>, age: Option<&str>, received: DateTime<Utc>) -> DateTime<Utc> {
    let Some(date) = date.and_then(|d| DateTime::parse_from_rfc2822(d).ok()) else {
        return received;
    };
    let date = date.with_timezone(&Utc);
    // a negative or absurd Age counts as 0, never a panic
    let age = age
        .and_then(|a| a.trim().parse::<u32>().ok())
        .and_then(|a| chrono::TimeDelta::try_seconds(a.into()))
        .unwrap_or_default();
    date.checked_sub_signed(age).unwrap_or(received)
}

#[derive(Deserialize)]
struct Body {
    data: Option<serde_json::Value>,
    #[serde(default)]
    errors: Vec<ErrorEntry>,
}

#[derive(Deserialize)]
struct ErrorEntry {
    message: String,
    #[serde(default)]
    extensions: Option<ErrorExtensions>,
}

#[derive(Deserialize)]
struct ErrorExtensions {
    code: Option<String>,
}

const UNKNOWN_HASH_CODES: [&str; 2] = ["PERSISTED_QUERY_NOT_IN_LIST", "PERSISTED_QUERY_NOT_FOUND"];

/// Any `errors` entry is a failure, also when `data` is there
pub fn parse_body<Op: Operation>(status: u16, body: &str) -> Result<Op::Data, GqlError> {
    let ok_status = (200..300).contains(&status);
    let parsed: Body = match serde_json::from_str(body) {
        Ok(b) => b,
        Err(_) if !ok_status => return Err(GqlError::Http { status }),
        Err(e) => return Err(GqlError::Parse(e)),
    };
    if let Some(error) = parsed.errors.into_iter().next() {
        let code = error.extensions.and_then(|e| e.code);
        if code
            .as_deref()
            .is_some_and(|c| UNKNOWN_HASH_CODES.contains(&c))
        {
            return Err(GqlError::UnknownHash { op: Op::NAME });
        }
        return Err(GqlError::Graphql {
            code,
            message: error.message,
        });
    }
    if !ok_status {
        return Err(GqlError::Http { status });
    }
    let data = parsed.data.ok_or_else(|| GqlError::Graphql {
        code: None,
        message: "answer without data".into(),
    })?;
    serde_json::from_value(data).map_err(GqlError::Parse)
}

/// The client: shared HTTP client + manifest
#[derive(Debug)]
pub struct Gql {
    http: &'static reqwest::Client,
    manifest: manifest::Store,
}

impl Gql {
    pub fn new(http: &'static reqwest::Client, manifest: manifest::Store) -> Self {
        Self { http, manifest }
    }

    /// On an unknown hash: refresh the manifest (once an hour at most) and try again once
    pub async fn call<Op: Operation>(&self, vars: &Op::Vars) -> Result<Answer<Op::Data>, GqlError> {
        match self.call_once::<Op>(vars).await {
            Err(GqlError::UnknownHash { op }) => {
                // retry even if another task did the refresh (Ok(false)): the manifest may be new now
                self.manifest.refresh_if_due(self.http).await?;
                self.call_once::<Op>(vars).await.inspect_err(|e| {
                    if matches!(e, GqlError::UnknownHash { .. }) {
                        log(format!("GQL_MANIFEST: {op} unknown hash after refresh"));
                    }
                })
            }
            other => other,
        }
    }

    async fn call_once<Op: Operation>(
        &self,
        vars: &Op::Vars,
    ) -> Result<Answer<Op::Data>, GqlError> {
        let (hash, client_version) = self.manifest.lookup(Op::NAME).await?;
        let params = query_params::<Op>(&hash, vars)?;
        let mut retries = 2;
        let response = loop {
            let sent = self
                .http
                .get(GQL_URL)
                .query(&params)
                // the API refuses calls without the client headers and a lolesports.com Referer (see the GraphQL investigation)
                .header("apollographql-client-name", CLIENT_NAME)
                .header("apollographql-client-version", &client_version)
                .header(reqwest::header::REFERER, format!("{SITE_URL}/"))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::USER_AGENT, user_agent())
                .send()
                .await;
            match sent {
                Ok(response) => break response,
                Err(e) if e.is_timeout() && retries > 0 => {
                    retries -= 1;
                    log(format!("Request to {} timed out", Op::NAME));
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Err(e) => return Err(GqlError::Network(e)),
            }
        };
        let received = Utc::now();
        let status = response.status().as_u16();
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let date = header(reqwest::header::DATE);
        let age = header(reqwest::header::AGE);
        let body = response.text().await.map_err(GqlError::Network)?;
        let data = parse_body::<Op>(status, &body)?;
        Ok(Answer {
            data,
            made_at: made_at(date.as_deref(), age.as_deref(), received),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    struct TestOp;
    impl Operation for TestOp {
        const NAME: &'static str = "GetOtherLeagues";
        type Vars = serde_json::Value;
        type Data = serde_json::Value;
    }

    #[test]
    fn params_are_encoded() {
        let vars = serde_json::json!({"locale": "en-US", "sport": "lol"});
        let params = query_params::<TestOp>("abc123", &vars).unwrap();
        let req = reqwest::Client::new()
            .get(GQL_URL)
            .query(&params)
            .build()
            .unwrap();
        let url = req.url();
        assert_eq!(url.as_str().split('?').next(), Some(GQL_URL));
        // encoded, never raw JSON in the URL (Bruno broke without it)
        assert!(!url.query().unwrap().contains('{'));
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs["operationName"], "GetOtherLeagues");
        let v: serde_json::Value = serde_json::from_str(&pairs["variables"]).unwrap();
        assert_eq!(v, vars);
        let e: serde_json::Value = serde_json::from_str(&pairs["extensions"]).unwrap();
        assert_eq!(
            e,
            serde_json::json!({"persistedQuery": {"version": 1, "sha256Hash": "abc123"}})
        );
    }

    #[test]
    fn made_at_is_date_minus_age() {
        let received = Utc.with_ymd_and_hms(2026, 10, 16, 23, 27, 10).unwrap();
        let t = made_at(Some("Fri, 16 Oct 2026 23:27:09 GMT"), Some("32"), received);
        assert_eq!(t, Utc.with_ymd_and_hms(2026, 10, 16, 23, 26, 37).unwrap());
        let t = made_at(Some("Fri, 16 Oct 2026 23:27:09 GMT"), None, received);
        assert_eq!(t, Utc.with_ymd_and_hms(2026, 10, 16, 23, 27, 9).unwrap());
        assert_eq!(made_at(Some("yesterday"), Some("5"), received), received);
        assert_eq!(made_at(None, Some("5"), received), received);
        let d = Utc.with_ymd_and_hms(2026, 10, 16, 23, 27, 9).unwrap();
        assert_eq!(
            made_at(
                Some("Fri, 16 Oct 2026 23:27:09 GMT"),
                Some("99999999999999"),
                received
            ),
            d
        );
        assert_eq!(
            made_at(Some("Fri, 16 Oct 2026 23:27:09 GMT"), Some("-5"), received),
            d
        );
    }

    #[test]
    fn body_ok() {
        let data = parse_body::<TestOp>(200, r#"{"data":{"leagues":[]}}"#).unwrap();
        assert_eq!(data, serde_json::json!({"leagues": []}));
    }

    #[test]
    fn body_unknown_hash() {
        // real answer for a made-up hash, 2026-10-17
        let body = r#"{"errors":[{"message":"Persisted query 'da7d' not found in the persisted query list","extensions":{"operation_name":"GetOtherLeagues","code":"PERSISTED_QUERY_NOT_IN_LIST"}}]}"#;
        assert!(matches!(
            parse_body::<TestOp>(404, body),
            Err(GqlError::UnknownHash {
                op: "GetOtherLeagues"
            })
        ));
        let body = r#"{"errors":[{"message":"PersistedQueryNotFound","extensions":{"code":"PERSISTED_QUERY_NOT_FOUND"}}]}"#;
        assert!(matches!(
            parse_body::<TestOp>(200, body),
            Err(GqlError::UnknownHash { .. })
        ));
    }

    #[test]
    fn body_other_errors() {
        let body = r#"{"errors":[{"message":"This endpoint does not allow freeform GraphQL requests","extensions":{"code":"PERSISTED_QUERY_ID_REQUIRED"}}]}"#;
        match parse_body::<TestOp>(400, body) {
            Err(GqlError::Graphql { code, .. }) => {
                assert_eq!(code.as_deref(), Some("PERSISTED_QUERY_ID_REQUIRED"))
            }
            other => panic!("{other:?}"),
        }
        // data and errors together is a failure
        let body = r#"{"data":{"leagues":[]},"errors":[{"message":"partial"}]}"#;
        assert!(matches!(
            parse_body::<TestOp>(200, body),
            Err(GqlError::Graphql { code: None, .. })
        ));
        assert!(matches!(
            parse_body::<TestOp>(401, "Unauthorized"),
            Err(GqlError::Http { status: 401 })
        ));
        assert!(matches!(
            parse_body::<TestOp>(200, "<html>"),
            Err(GqlError::Parse(_))
        ));
    }
}
