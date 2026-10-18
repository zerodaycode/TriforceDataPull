//! `GetSeasonTournament`: tournaments by id with their stage names, in one locale

use serde::{Deserialize, Serialize};

use crate::client::gql::Operation;

pub struct GetSeasonTournament;

impl Operation for GetSeasonTournament {
    const NAME: &'static str = "GetSeasonTournament";
    type Vars = Vars;
    type Data = Data;
}

/// Ids per call: the ids go in the URL, 514 at once is HTTP 414
pub const MAX_IDS: usize = 100;

/// `query GetSeasonTournament($hl: Locale!, $tournamentId: [ID!])`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vars {
    pub hl: String,
    pub tournament_id: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Data {
    #[serde(default, deserialize_with = "super::null_as_empty")]
    pub tournaments: Vec<Tournament>,
}

/// Only the stages: the answer also has teams, sections and matches, unused for now
#[derive(Debug, Deserialize)]
pub struct Tournament {
    #[serde(default, deserialize_with = "super::null_as_empty")]
    pub stages: Vec<Stage>,
}

#[derive(Debug, Deserialize)]
pub struct Stage {
    pub id: String,
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::parse_body;

    #[test]
    fn vars() {
        let v = Vars {
            hl: "es-ES".into(),
            tournament_id: vec!["1".into(), "2".into()],
        };
        assert_eq!(
            serde_json::to_value(v).unwrap(),
            serde_json::json!({"hl": "es-ES", "tournamentId": ["1", "2"]})
        );
        assert_eq!(GetSeasonTournament::NAME, "GetSeasonTournament");
    }

    #[test]
    fn stages_with_names() {
        let body = r#"{"data":{"tournaments":[
            {"__typename":"Tournament","id":"115826472385208906","name":"LFL INVITATIONAL","teams":[],
             "stages":[{"__typename":"Stage","id":"115826504485263848","name":"Groups","slug":"groups","sections":[]},
                       {"__typename":"Stage","id":"115826525499644450","name":"Playoffs","slug":"playoffs","sections":null}]},
            {"__typename":"Tournament","id":"1","stages":null}]}}"#;
        let data = parse_body::<GetSeasonTournament>(200, body).unwrap();
        assert_eq!(data.tournaments.len(), 2);
        assert_eq!(
            data.tournaments[0].stages[1].name.as_deref(),
            Some("Playoffs")
        );
        assert!(data.tournaments[1].stages.is_empty());
    }
}
