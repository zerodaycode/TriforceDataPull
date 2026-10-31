//! `watchLiveQuery`: every match and show live right now, with the side of each team per game

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    schedule_events::{League, Stream, TeamResult, Tournament},
    watch_vod::GameTeam,
};
use crate::client::gql::Operation;

pub struct GetWatchLive;

impl Operation for GetWatchLive {
    const NAME: &'static str = "watchLiveQuery";
    type Vars = Vars;
    type Data = Data;
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vars {
    pub hl: String,
    pub sport: Vec<String>,
    pub event_state: Vec<String>,
    pub event_type: String,
    pub page_size: u32,
}

impl Default for Vars {
    fn default() -> Self {
        Self {
            hl: "en-US".into(),
            sport: vec!["lol".into()],
            event_state: vec!["inProgress".into()],
            event_type: "all".into(),
            page_size: 100,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Data {
    pub esports: Esports,
}

#[derive(Debug, Deserialize)]
pub struct Esports {
    #[serde(default, deserialize_with = "super::each_event")]
    pub events: Vec<LiveEvent>,
}

/// No `state` in this query: every event in the answer is live
#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
pub enum LiveEvent {
    #[serde(rename = "EventMatch")]
    Match(LiveMatch),
    #[serde(rename = "EventShow")]
    Show(LiveShow),
    /// an event that didn't decode: only it is lost
    #[serde(skip_deserializing)]
    Bad(String),
    #[serde(other)]
    Other,
}

impl super::BadEvent for LiveEvent {
    fn bad(why: String) -> Self {
        LiveEvent::Bad(why)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveMatch {
    pub id: String,
    pub block_name: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub league: League,
    pub tournament: Option<Tournament>,
    pub r#match: Match,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub streams: Vec<Stream>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveShow {
    pub id: String,
    pub start_time: Option<DateTime<Utc>>,
    pub league: League,
    pub tournament: Option<Tournament>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub streams: Vec<Stream>,
}

/// No `type` (normal/tiebreaker) in this query
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub strategy: Option<super::schedule_events::Strategy>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub games: Vec<Game>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub match_teams: Vec<MatchTeam>,
}

#[derive(Debug, Deserialize)]
pub struct Game {
    pub id: String,
    pub number: i32,
    pub state: String,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub teams: Vec<GameTeam>,
}

#[derive(Debug, Deserialize)]
pub struct MatchTeam {
    /// `<matchId>:<teamId>`, no plain `teamId` in this query
    pub id: String,
    pub code: String,
    pub name: String,
    pub image: Option<String>,
    pub result: Option<TeamResult>,
}

impl MatchTeam {
    /// The team id after the `:`
    pub fn team_id(&self) -> &str {
        self.id.rsplit_once(':').map_or(&self.id, |(_, t)| t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::parse_body;

    #[test]
    fn vars() {
        assert_eq!(
            serde_json::to_value(Vars::default()).unwrap(),
            serde_json::json!({"hl": "en-US", "sport": ["lol"], "eventState": ["inProgress"],
                "eventType": "all", "pageSize": 100})
        );
        assert_eq!(GetWatchLive::NAME, "watchLiveQuery");
    }

    #[test]
    fn parses_the_fixture() {
        let data = parse_body::<GetWatchLive>(200, fixture!("gql/watch_live.json")).unwrap();
        let ev = &data.esports.events;
        assert_eq!(ev.len(), 4);
        let LiveEvent::Match(m) = &ev[0] else {
            panic!("a match")
        };
        assert_eq!(m.r#match.match_teams[0].team_id(), "100205576309502431");
        assert_eq!(m.r#match.games[1].teams[0].side.as_deref(), Some("red"));
        assert!(matches!(ev[1], LiveEvent::Show(_)));
        assert!(matches!(&ev[2], LiveEvent::Bad(s) if s.starts_with("9:")));
        assert!(matches!(ev[3], LiveEvent::Other));
    }

    #[test]
    fn nothing_live() {
        let data = parse_body::<GetWatchLive>(200, r#"{"data":{"esports":{"events":[]}}}"#);
        assert!(data.unwrap().esports.events.is_empty());
        let data = parse_body::<GetWatchLive>(200, r#"{"data":{"esports":{"events":null}}}"#);
        assert!(data.unwrap().esports.events.is_empty());
    }
}
