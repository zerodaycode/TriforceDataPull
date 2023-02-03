///! Flow of requests for the LoLEsport API and data manipulation
/// For more information about the API (endpoints, parameters, etc) visit https://vickz84259.github.io/lolesports-api-docs/#tag/match-details
/// 
/// First request -> Ask for leagues
/// 
/// Second request -> Ask for tournaments
/// This request must be done by league ID, so we can identify the tournaments for each league
/// 
/// Third request -> Ask for teams
/// 
/// Fourth request -> Ask for players
/// 
/// It is important to note that both teams and players are obtained from the same endpoint
/// 
/// Fifth request -> Ask for schedule
/// 
/// We will insert data with the upsert method, and deleting deprecated data if needed
/// 

pub mod serde_models {
    use std::collections::HashMap;

    use canyon_sql::db_clients::tiberius::time::chrono;
    use serde::{Deserialize, Deserializer, Serialize};


    #[derive(Deserialize, Debug)]
    pub struct Wrapper<T> {
        pub data: T,
    }

    #[derive(Deserialize, Debug, Default)]
    pub struct Leagues {
        pub leagues: Vec<League>
    }


    #[derive(Deserialize, Debug)]
    pub struct League {
        pub id: LolesportsId,
        pub slug: String,
        pub name: String,
        pub region: String,
        pub image: String,
    }

    #[derive(Deserialize, Default, Debug, Clone)]
    pub struct LeagueForTournaments {
        pub leagues: Vec<Tournaments>
    }

    #[derive(Deserialize, Debug, Default, Clone)]
    pub struct Tournaments {
        #[serde(skip)]
        pub league_id: LolesportsId,
        pub tournaments: Vec<Tournament>
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Tournament {
        id: LolesportsId,
        slug: String,
        #[serde(alias = "startDate")]
        start_date: chrono::NaiveDate,
        #[serde(alias = "endDate")]
        end_date: chrono::NaiveDate,
    }

    #[derive(Debug, Default, Serialize, Clone)]
    pub struct LolesportsId(i64);
    impl<'de> Deserialize<'de> for LolesportsId {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s: &str = Deserialize::deserialize(deserializer)?;
            Ok(LolesportsId(
                s.parse::<i64>().expect(
                    &format!("Failed to deserialize the Lolesports id: {s:?}")
                )
            ))
        }
    }

    #[derive(Deserialize)]
    pub struct Player {
        id: i64,
        #[serde(alias = "firstName")]
        first_name: String,
        #[serde(alias = "lastName")]
        last_name: String,
        #[serde(alias = "summonerName")]
        summoner_name: String,
        image: Option<String>,
        role: String
    }

    #[derive(Deserialize)]
    pub struct Team {
        id: i64,
        name: String,
        slug: String,
        code: String,
        image: String,
        #[serde(alias = "alternativeImage")]
        alternative_image: Option<String>,
        #[serde(alias = "backgroundImage")]
        background_image: Option<String>,
        status: String,
        players: Vec<Player>,
        #[serde(alias = "homeLeague")]
        home_league: League
    }

    #[derive(Deserialize)]
    pub struct Event {
        #[serde(alias = "startTime")]
        start_time: chrono::NaiveDateTime,
        state: String,
        r#type: String,
        #[serde(alias = "blockName")]
        block_name: String,
        league: League,
        teams: Vec<TeamEvent>,
        strategy: Strategy
    }

    #[derive(Deserialize)]
    pub struct TeamEvent {
        team: Team,
        result: MatchTeamResult
    }

    #[derive(Deserialize)]
    pub struct MatchTeamResult {
        outcome: String,
        #[serde(alias = "gameWins")]
        game_wins: String
    }

    #[derive(Deserialize)]
    pub struct Strategy {
        r#type: String,
        count: i8
    }

}



/// Autonomous process triggered every (X config data [replace this])
/// that queries the lolesports API to fetch data and sync the received
/// data with the one stored in our PostgreSQL
fn scheduled_data_pull_process() {

}

/// Manages the operations needed to query lolespors
fn retrieve_lolesports_data() {
    // Stack of function calls to actually retrieve the data
}

/// Responsable for retrieving the [`crate::`]
fn get_lolesport_leagues() {

}

fn get_lolesport_tournaments() {

}

fn get_lolesport_teams() {

}

fn get_lolesport_schedule() {

}

