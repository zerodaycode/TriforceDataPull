//! `scheduleEvents`: matches and shows by date, in pages, in one locale

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::client::gql::Operation;

pub struct GetScheduleEvents;

impl Operation for GetScheduleEvents {
    const NAME: &'static str = "scheduleEvents";
    type Vars = Vars;
    type Data = Data;
}

/// The site asks up to 300, 100 keeps an answer around 600 KB
pub const PAGE_SIZE: u32 = 100;

/// `eventDateStart` alone is refused ("Invalid eventDateStart/eventDateEnd"): always both
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vars {
    pub hl: String,
    pub sport: Vec<String>,
    pub event_type: String,
    pub page_size: u32,
    /// `YYYY-MM-DD`
    pub event_date_start: String,
    pub event_date_end: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
}

impl Vars {
    pub fn new(hl: &str, start: &str, end: &str, page_token: Option<String>) -> Self {
        Self {
            hl: hl.into(),
            sport: vec!["lol".into()],
            event_type: "all".into(),
            page_size: PAGE_SIZE,
            event_date_start: start.into(),
            event_date_end: end.into(),
            page_token,
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
    pub events: Vec<Event>,
    pub pages: Pages,
}

#[derive(Debug, Deserialize)]
pub struct Pages {
    pub newer: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
pub enum Event {
    #[serde(rename = "EventMatch")]
    Match(EventMatch),
    #[serde(rename = "EventShow")]
    Show(EventShow),
    /// an event that didn't decode (a `null` where a value is required): only it is lost
    #[serde(skip_deserializing)]
    Bad(String),
    /// a type the query doesn't ask fields for
    #[serde(other)]
    Other,
}

impl super::BadEvent for Event {
    fn bad(why: String) -> Self {
        Event::Bad(why)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventMatch {
    pub id: String,
    pub block_name: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub state: String,
    pub league: League,
    pub tournament: Option<Tournament>,
    pub r#match: Match,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub match_teams: Vec<MatchTeam>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub streams: Vec<Stream>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventShow {
    pub id: String,
    pub start_time: Option<DateTime<Utc>>,
    pub state: String,
    pub league: League,
    pub tournament: Option<Tournament>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub streams: Vec<Stream>,
}

#[derive(Debug, Deserialize)]
pub struct League {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub image: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Tournament {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct Match {
    /// `normal` or `tiebreaker`
    pub r#type: Option<String>,
    pub strategy: Option<Strategy>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub games: Vec<Game>,
}

#[derive(Debug, Deserialize)]
pub struct Strategy {
    /// `bestOf` or `playAll`
    pub r#type: String,
    pub count: i32,
}

#[derive(Debug, Deserialize)]
pub struct Game {
    pub id: String,
    pub number: i32,
    pub state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchTeam {
    /// `"0"` for a slot not decided yet (TBD)
    pub team_id: String,
    pub code: String,
    pub name: String,
    pub image: Option<String>,
    pub result: Option<TeamResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamResult {
    pub game_wins: Option<i32>,
    pub outcome: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    pub provider: String,
    pub parameter: String,
    pub media_locale: Option<MediaLocale>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaLocale {
    pub locale: String,
    pub english_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::parse_body;

    #[test]
    fn vars() {
        let v = Vars::new("en-US", "2023-10-01", "2027-10-18", None);
        assert_eq!(
            serde_json::to_value(v).unwrap(),
            serde_json::json!({"hl": "en-US", "sport": ["lol"], "eventType": "all", "pageSize": 100,
                "eventDateStart": "2023-10-01", "eventDateEnd": "2027-10-18"})
        );
        let v = Vars::new("es-ES", "a", "b", Some("tok".into()));
        assert_eq!(serde_json::to_value(v).unwrap()["pageToken"], "tok");
        assert_eq!(GetScheduleEvents::NAME, "scheduleEvents");
    }

    #[test]
    fn parses_the_fixture() {
        let data =
            parse_body::<GetScheduleEvents>(200, fixture!("gql/schedule_events_en.json")).unwrap();
        let matches: Vec<&EventMatch> = data
            .esports
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Match(m) => Some(m),
                _ => None,
            })
            .collect();
        assert_eq!(matches.len(), 5);
        // TBD: both slots "0", no result yet
        let tbd = matches[0];
        assert_eq!(tbd.state, "unstarted");
        assert!(tbd.match_teams.iter().all(|t| t.team_id == "0"));
        // Bo5 3-1 with game 5 unneeded
        let bo5 = matches[2];
        assert_eq!(bo5.r#match.strategy.as_ref().unwrap().count, 5);
        assert_eq!(bo5.r#match.games[4].state, "unneeded");
        let wins: Vec<_> = bo5
            .match_teams
            .iter()
            .map(|t| t.result.as_ref().unwrap().game_wins)
            .collect();
        assert_eq!(wins, [Some(3), Some(1)]);
        assert_eq!(matches[3].r#match.r#type.as_deref(), Some("tiebreaker"));
        assert_eq!(
            matches[4].r#match.strategy.as_ref().unwrap().r#type,
            "playAll"
        );
        assert!(data.esports.pages.newer.is_none());
    }

    #[test]
    fn nulls_and_other_types() {
        let body = r#"{"data":{"esports":{"events":[
            {"__typename":"EventMatch","id":"1","state":"unstarted","startTime":null,"blockName":null,
             "league":{"id":"2","slug":"l","name":"L","image":null},"tournament":null,
             "match":{"games":null,"strategy":null},"matchTeams":null,"streams":null},
            {"__typename":"EventShow","id":"3","state":"completed","startTime":"2026-01-01T10:00:00Z",
             "league":{"id":"2","slug":"l","name":"L","image":"i"},"tournament":{"id":"4"},"streams":null},
            {"__typename":"EventSomethingNew","id":"5"},
            {"__typename":"EventMatch","id":"6","state":"completed","league":null,"match":{}}],
            "pages":{"older":null,"newer":"next"}}}}"#;
        let data = parse_body::<GetScheduleEvents>(200, body).unwrap();
        assert_eq!(data.esports.events.len(), 4);
        assert!(matches!(&data.esports.events[3], Event::Bad(e) if e.starts_with("6: ")));
        let Event::Match(m) = &data.esports.events[0] else {
            panic!("a match")
        };
        assert!(m.r#match.games.is_empty() && m.match_teams.is_empty() && m.streams.is_empty());
        assert!(matches!(data.esports.events[1], Event::Show(_)));
        assert!(matches!(data.esports.events[2], Event::Other));
        assert_eq!(data.esports.pages.newer.as_deref(), Some("next"));
    }
}
