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
    use std::fmt::Display;

    use canyon_sql::{date_time::NaiveDateTime, db_clients::tiberius::time::chrono};

    use serde::{Deserialize, Deserializer, Serialize};

    #[derive(Deserialize, Debug)]
    pub struct Wrapper<T> {
        pub data: T,
    }

    #[derive(Deserialize, Debug, Default)]
    pub struct Leagues {
        pub leagues: Vec<League>,
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
        pub leagues: Vec<Tournaments>,
    }

    #[derive(Deserialize, Debug, Default, Clone)]
    pub struct Tournaments {
        pub tournaments: Vec<Tournament>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Tournament {
        pub id: LolesportsId,
        pub slug: String,
        #[serde(alias = "startDate")]
        pub start_date: chrono::NaiveDate,
        #[serde(alias = "endDate")]
        pub end_date: chrono::NaiveDate,
        #[serde(skip)]
        pub league_id: LolesportsId,
    }

    #[derive(Debug, Default, Serialize, Clone, Copy)]
    pub struct LolesportsId(pub i64);
    impl<'de> Deserialize<'de> for LolesportsId {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s: &str = Deserialize::deserialize(deserializer)?;
            Ok(LolesportsId(s.parse::<i64>().unwrap_or_else(|_| {
                panic!("Failed to deserialize the Lolesports id: {s:?}")
            })))
        }
    }

    impl From<LolesportsId> for i64 {
        fn from(value: LolesportsId) -> Self {
            value.0
        }
    }

    #[derive(Debug, Default, Serialize, Clone, Copy)]
    pub struct LolesportsDateTime(pub NaiveDateTime);
    impl<'de> Deserialize<'de> for LolesportsDateTime {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s: &str = Deserialize::deserialize(deserializer)?;
            Ok(LolesportsDateTime(
                NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.3fZ").unwrap_or_else(|_| {
                    panic!("Failed to deserialize the Lolesports DateTime: {s:?}")
                }),
            ))
        }
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Player {
        pub id: LolesportsId,
        #[serde(alias = "firstName")]
        pub first_name: String,
        #[serde(alias = "lastName")]
        pub last_name: String,
        #[serde(alias = "summonerName")]
        pub summoner_name: String,
        pub image: Option<String>,
        pub role: String,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct TeamsPlayers {
        pub teams: Vec<Team>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Team {
        pub id: LolesportsId,
        pub name: String,
        pub slug: String,
        pub code: String,
        pub image: String,
        #[serde(alias = "alternativeImage")]
        pub alternative_image: Option<String>,
        #[serde(alias = "backgroundImage")]
        pub background_image: Option<String>,
        pub status: String,
        pub players: Vec<Player>,
        #[serde(alias = "homeLeague")]
        pub home_league: Option<HomeLeague>,
    }

    impl Display for Team {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{:?} {} {} {:#?}",
                self.id, self.name, self.status, self.home_league
            )
        }
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct HomeLeague {
        #[serde(skip)]
        pub league_id: LolesportsId,
        pub name: String,
        pub region: String,
    }

    #[derive(Deserialize, Default, Debug)]
    pub struct ScheduleOutter {
        pub schedule: Schedule,
    }

    #[derive(Deserialize, Default, Debug)]
    pub struct Schedule {
        #[serde(default)]
        pub pages: Pages,
        pub events: Vec<Event>,
    }

    #[derive(Deserialize, Default, Debug)]
    pub struct Pages {
        pub older: Option<String>,
        pub newer: Option<String>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct ScheduleLeague {
        #[serde(default)]
        pub league_id: LolesportsId,
        pub name: String,
        pub slug: String,
    }

    #[derive(Deserialize, Debug)]
    pub struct Event {
        #[serde(alias = "startTime")]
        pub start_time: LolesportsDateTime,
        pub state: String,
        pub r#type: String,
        #[serde(alias = "blockName")]
        pub block_name: Option<String>,
        pub league: ScheduleLeague,
        #[serde(default)]
        pub r#match: Option<Match>,
    }

    #[derive(Deserialize, Debug)]
    pub struct Match {
        pub id: LolesportsId,
        pub teams: Vec<TeamEvent>,
        pub strategy: Strategy,
    }

    #[derive(Deserialize, Debug)]
    pub struct TeamEvent {
        pub name: String,
        pub code: String,
        pub image: String,
        pub result: Option<MatchTeamResult>,
    }

    #[derive(Deserialize, Debug)]
    pub struct MatchTeamResult {
        pub outcome: Option<String>,
        #[serde(alias = "gameWins")]
        pub game_wins: i8,
    }

    #[derive(Deserialize, Debug)]
    pub struct Strategy {
        pub r#type: String,
        pub count: i8,
    }
}
