//! `watchVodQuery`: one event with its games and the side of each team. Only for the games the
//! feed has nothing for

use serde::{Deserialize, Serialize};

use crate::client::gql::Operation;

pub struct GetWatchVod;

impl Operation for GetWatchVod {
    const NAME: &'static str = "watchVodQuery";
    type Vars = Vars;
    type Data = Data;
}

/// `query watchVodQuery($eventId: ID!, $hl: Locale!, $vodType: [VodType])`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vars {
    pub event_id: String,
    pub hl: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Data {
    pub scheduled_event: Option<VodEvent>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
pub enum VodEvent {
    #[serde(rename = "EventMatch")]
    Match(VodMatchEvent),
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct VodMatchEvent {
    pub r#match: VodMatch,
}

#[derive(Debug, Deserialize)]
pub struct VodMatch {
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub games: Vec<VodGame>,
}

#[derive(Debug, Deserialize)]
pub struct VodGame {
    pub id: String,
    pub state: String,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub teams: Vec<GameTeam>,
}

/// `id` is the plain team id here (not `<match>:<team>`)
#[derive(Debug, Deserialize)]
pub struct GameTeam {
    pub id: String,
    /// not picked yet: `null`, or missing
    #[serde(default)]
    pub side: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::parse_body;

    #[test]
    fn vars() {
        let v = Vars {
            event_id: "1".into(),
            hl: "en-US".into(),
        };
        assert_eq!(
            serde_json::to_value(v).unwrap(),
            serde_json::json!({"eventId": "1", "hl": "en-US"})
        );
        assert_eq!(GetWatchVod::NAME, "watchVodQuery");
    }

    #[test]
    fn games_with_sides() {
        let data = parse_body::<GetWatchVod>(200, fixture!("gql/watch_vod.json")).unwrap();
        let Some(VodEvent::Match(e)) = data.scheduled_event else {
            panic!("a match")
        };
        assert_eq!(e.r#match.games.len(), 5);
        let g2 = &e.r#match.games[1];
        assert_eq!(g2.state, "completed");
        assert_eq!(
            g2.teams
                .iter()
                .map(|t| (t.id.as_str(), t.side.as_deref().unwrap_or("")))
                .collect::<Vec<_>>(),
            [("98767991877340524", "blue"), ("100205576309502431", "red")]
        );
    }

    #[test]
    fn show_and_null_teams() {
        let body = r#"{"data":{"scheduledEvent":{"__typename":"EventShow","id":"1"}}}"#;
        let data = parse_body::<GetWatchVod>(200, body).unwrap();
        assert!(matches!(data.scheduled_event, Some(VodEvent::Other)));
        let body = r#"{"data":{"scheduledEvent":{"__typename":"EventMatch","id":"1","match":{"games":[{"id":"2","number":1,"state":"completed","teams":null}]}}}}"#;
        let Some(VodEvent::Match(e)) = parse_body::<GetWatchVod>(200, body)
            .unwrap()
            .scheduled_event
        else {
            panic!("a match")
        };
        assert!(e.r#match.games[0].teams.is_empty());
    }
}
