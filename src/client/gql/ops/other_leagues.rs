//! `GetOtherLeagues`: every league with its tournaments, one call

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::client::gql::Operation;

pub struct GetOtherLeagues;

impl Operation for GetOtherLeagues {
    const NAME: &'static str = "GetOtherLeagues";
    type Vars = Vars;
    type Data = Data;
}

/// `query GetOtherLeagues($locale: Locale!, $sport: Sport)`. No flags: all 51 leagues, hidden ones too, like `getLeagues`
#[derive(Debug, Serialize)]
pub struct Vars {
    pub locale: String,
    pub sport: &'static str,
}

impl Default for Vars {
    fn default() -> Self {
        // en-US keeps `region` like we store it today
        Self {
            locale: "en-US".into(),
            sport: "lol",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Data {
    pub leagues: Vec<League>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct League {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub region: String,
    pub image: String,
    #[serde(default)]
    pub dark_image: Option<String>,
    #[serde(default)]
    pub display_priority: Option<DisplayPriority>,
    #[serde(default)]
    pub tournaments: Vec<Tournament>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tournament {
    pub id: String,
    pub slug: String,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub season: Option<Season>,
    #[serde(default)]
    pub split: Option<IdOnly>,
    #[serde(default)]
    pub stages: Vec<IdOnly>,
}

#[derive(Debug, Deserialize)]
pub struct DisplayPriority {
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Season {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct IdOnly {
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::{Operation, parse_body};

    #[test]
    fn vars() {
        assert_eq!(
            serde_json::to_value(Vars::default()).unwrap(),
            serde_json::json!({"locale": "en-US", "sport": "lol"})
        );
    }

    #[test]
    fn parses_the_real_answer() {
        let body = fixture!("gql/other_leagues.json");
        let data = parse_body::<GetOtherLeagues>(200, body).unwrap();
        assert_eq!(GetOtherLeagues::NAME, "GetOtherLeagues");
        assert_eq!(data.leagues.len(), 2);
        let lec = &data.leagues[1];
        assert_eq!(
            (lec.id.as_str(), lec.slug.as_str(), lec.region.as_str()),
            ("98767991302996019", "lec", "EMEA")
        );
        let t = &lec.tournaments[0];
        assert_eq!(t.slug, "lec_split_3_2026");
        assert_eq!(
            t.start_time.unwrap().to_rfc3339(),
            "2026-07-23T22:00:00+00:00"
        );
    }
}
