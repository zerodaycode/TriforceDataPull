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

    impl From<LolesportsDateTime> for NaiveDateTime {
        fn from(value: LolesportsDateTime) -> Self {
            value.0
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

    #[derive(Deserialize, Debug)]
    pub struct EventOutter {
        pub event: EventDetails,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct EventDetails {
        pub id: LolesportsId,
        pub r#type: String,
        pub state: Option<String>,
        pub blockname: Option<String>,
        #[serde(alias = "startTime")]
        pub start_time: Option<LolesportsDateTime>,
        pub tournament: EventDetailTournament,
        pub league: ScheduleLeague,
        pub r#match: Option<EventDetailMatch>,
        pub streams: Vec<Stream>,
    }

    impl PartialEq for EventDetails {
        fn eq(&self, other: &Self) -> bool {
            if self.id.0 != other.id.0 || self.state != other.state {
                return false;
            }
            if let (Some(self_match), Some(other_match)) = (&self.r#match, &other.r#match) {
                if self_match.teams.len() != other_match.teams.len() {
                    return false;
                }
                for (self_team, other_team) in self_match.teams.iter().zip(other_match.teams.iter())
                {
                    if self_team.result.is_some() && other_team.result.is_some() {
                        let self_result = self_team.result.as_ref().unwrap();
                        let other_result = other_team.result.as_ref().unwrap();

                        if self_result.game_wins != other_result.game_wins {
                            return false;
                        }
                    }
                    if self_team.name != other_team.name
                        || self_team.code != other_team.code
                        || self_team.image != other_team.image
                    {
                        return false;
                    }
                }

                for (self_game, other_game) in self_match.games.iter().zip(other_match.games.iter())
                {
                    if self_game.id != other_game.id
                        || self_game.state != other_game.state
                        || self_game.number != other_game.number
                    {
                        return false;
                    }
                }

                return true;
            } else {
                return (self.start_time.is_none() == other.start_time.is_none()
                    || self.start_time.is_some() && other.start_time.is_some())
                    && self.league.name == other.league.name
                    && self.r#type == other.r#type;
            }
        }
    }

    #[derive(Deserialize, Default, Debug)]
    pub struct ScheduleOutter {
        pub schedule: Schedule,
    }

    #[derive(Deserialize, Default, Debug)]
    pub struct LiveScheduleOutter {
        pub schedule: LiveSchedule,
    }

    #[derive(Deserialize, Default, Debug)]
    pub struct LiveSchedule {
        pub events: Vec<EventDetails>,
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

    #[derive(Deserialize, Debug, Clone)]
    pub struct Event {
        #[serde(alias = "startTime")]
        pub start_time: LolesportsDateTime,
        pub state: String,
        pub r#type: String,
        #[serde(alias = "blockName")]
        pub block_name: Option<String>,
        pub league: ScheduleLeague,
        #[serde(default)]
        slug: String,
        pub r#match: Option<Match>,
    }

    impl PartialEq for Event {
        fn eq(&self, other: &Self) -> bool {
            if let (Some(self_match), Some(other_match)) = (&self.r#match, &other.r#match) {
                if self_match.id.0 != other_match.id.0
                    || self_match.teams.len() != other_match.teams.len()
                {
                    return false;
                }
                for (self_team, other_team) in self_match.teams.iter().zip(other_match.teams.iter())
                {
                    if self_team.result.is_some() && other_team.result.is_some() {
                        let self_result = self_team.result.as_ref().unwrap();
                        let other_result = other_team.result.as_ref().unwrap();
                        if self_result.game_wins != other_result.game_wins {
                            return false;
                        }
                    }
                    if self_team.name != other_team.name
                        || self_team.code != other_team.code
                        || self_team.image != other_team.image
                    {
                        return false;
                    }
                }
                return true;
            } else {
                self.start_time.0 == other.start_time.0
                    && self.league.name == other.league.name
                    && self.state == other.state
                    && self.r#type == other.r#type
                    && self.block_name == other.block_name
                    && self.slug == other.slug
                    && other.r#match.is_none()
            }
        }
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Match {
        pub id: LolesportsId,
        pub teams: Vec<TeamEvent>,
        pub strategy: Strategy,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct EventDetailTournament {
        pub id: LolesportsId,
    }
    #[derive(Deserialize, Debug, Clone)]
    pub struct EventDetailMatch {
        pub teams: Vec<TeamEvent>,
        pub strategy: Strategy,
        #[serde(default)]
        pub games: Vec<Game>,
    }
    #[derive(Debug, Deserialize, Clone)]
    pub struct Game {
        pub number: i32,
        pub id: String,
        pub state: String,
        pub teams: Vec<TeamSide>,
        #[serde(default)]
        pub vods: Vec<Vod>,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct TeamSide {
        pub id: String,
        pub side: String,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct Vod {
        pub id: String,
        pub parameter: String,
        pub locale: String,
        #[serde(alias = "mediaLocale")]
        pub media_locale: MediaLocale,
        pub provider: String,
        pub offset: i32,
        #[serde(alias = "firstFrameTime")]
        pub first_frame_time: String,
        #[serde(alias = "startMillis")]
        pub start_millis: Option<i64>,
        #[serde(alias = "endMillis")]
        pub end_millis: Option<i64>,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct Stream {
        parameter: String,
        locale: String,
        #[serde(alias = "mediaLocale")]
        media_locale: MediaLocale,
        provider: String,
        countries: Vec<String>,
        offset: i32,
        #[serde(alias = "statsStatus")]
        stats_status: String,
    }

    #[derive(Debug, Deserialize, Clone)]
    pub struct MediaLocale {
        pub locale: String,
        #[serde(alias = "englishName")]
        pub english_name: String,
        #[serde(alias = "translatedName")]
        pub translated_name: String,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct TeamEvent {
        pub name: String,
        pub code: String,
        pub image: String,
        pub result: Option<MatchTeamResult>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct MatchTeamResult {
        pub outcome: Option<String>,
        #[serde(alias = "gameWins")]
        pub game_wins: i8,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Strategy {
        #[serde(default)]
        pub r#type: String,
        pub count: i8,
    }
}
