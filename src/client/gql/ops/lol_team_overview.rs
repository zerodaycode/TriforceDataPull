//! `GetLoLTeamOverview` without `teamId`: every team with its players

use serde::{Deserialize, Serialize};

use crate::client::gql::Operation;

pub struct GetLoLTeamOverview;

impl Operation for GetLoLTeamOverview {
    const NAME: &'static str = "GetLoLTeamOverview";
    type Vars = Vars;
    type Data = Data;
}

/// No `teamId`: all teams, like the old `getTeams`
#[derive(Debug, Serialize)]
pub struct Vars {
    pub hl: &'static str,
    pub sport: &'static str,
}

impl Default for Vars {
    fn default() -> Self {
        Self {
            hl: "en-US",
            sport: "lol",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Data {
    pub teams: Vec<Team>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub code: String,
    pub image: String,
    pub background_image: Option<String>,
    pub home_league: Option<HomeLeague>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub players: Vec<Player>,
}

/// GraphQL sends `null`, not a missing key: treat it as empty
fn null_as_empty<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
pub struct HomeLeague {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub summoner_name: String,
    pub role: String,
    pub image: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::{Operation, parse_body};

    #[test]
    fn vars() {
        assert_eq!(
            serde_json::to_value(Vars::default()).unwrap(),
            serde_json::json!({"hl": "en-US", "sport": "lol"})
        );
        assert_eq!(GetLoLTeamOverview::NAME, "GetLoLTeamOverview");
    }

    #[test]
    fn parses_the_real_answer() {
        let body = include_str!("../../adapters/testdata/lol_team_overview.json");
        let data = parse_body::<GetLoLTeamOverview>(200, body).unwrap();
        assert_eq!(data.teams.len(), 3);
        let t1 = &data.teams[0];
        assert_eq!((t1.code.as_str(), t1.players.len()), ("T1", 2));
        assert_eq!(t1.home_league.as_ref().unwrap().id, "98767991310872058");
        assert_eq!(t1.players[0].summoner_name, "Guardian");
        assert!(data.teams[2].home_league.is_none());
    }
}
