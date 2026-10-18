//! What the v2 DAO takes. Adapters build these from the API, the DAO never sees API types

use canyon_sql::date_time::NaiveDateTime;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReferenceRows {
    pub leagues: Vec<LeagueRow>,
    pub seasons: Vec<SeasonRow>,
    pub splits: Vec<i64>,
    pub tournaments: Vec<TournamentRow>,
    pub stages: Vec<StageRow>,
    pub teams: Vec<TeamRow>,
    pub players: Vec<PlayerRow>,
    /// (team ext_id, player ext_id), the whole current roster
    pub team_players: Vec<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeagueRow {
    pub ext_id: i64,
    pub slug: String,
    pub image_url: String,
    pub dark_image_url: Option<String>,
    pub texts: Vec<LeagueText>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeagueText {
    pub locale: String,
    pub name: String,
    pub region: String,
    pub position: Option<i32>,
}

/// A translated name
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub locale: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeasonRow {
    pub ext_id: i64,
    pub start_time: Option<NaiveDateTime>,
    pub end_time: Option<NaiveDateTime>,
    pub texts: Vec<Text>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TournamentRow {
    pub ext_id: i64,
    pub league_ext_id: i64,
    pub season_ext_id: Option<i64>,
    pub split_ext_id: Option<i64>,
    pub slug: String,
    pub start_time: Option<NaiveDateTime>,
    pub end_time: Option<NaiveDateTime>,
    pub texts: Vec<Text>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageRow {
    pub ext_id: i64,
    pub tournament_ext_id: i64,
    pub texts: Vec<Text>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamRow {
    pub ext_id: i64,
    pub slug: String,
    pub name: String,
    pub code: String,
    pub image_url: String,
    pub bg_image_url: Option<String>,
    pub home_league_ext_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRow {
    pub ext_id: i64,
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub image_url: Option<String>,
    pub role: String,
}

/// One `scheduleEvents` page in one locale
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventRows {
    pub events: Vec<EventRow>,
    /// Leagues and teams seen in the events, only added when we don't have them
    pub league_stubs: Vec<LeagueStub>,
    pub team_stubs: Vec<TeamStub>,
    /// Events left out by the adapter (bad id, unknown type)
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventRow {
    pub ext_id: i64,
    /// `match` or `show`
    pub kind: &'static str,
    pub league_ext_id: i64,
    pub tournament_ext_id: Option<i64>,
    pub start_time: Option<NaiveDateTime>,
    pub state: String,
    pub strategy_type: Option<String>,
    pub strategy_count: Option<i32>,
    pub match_type: Option<String>,
    /// In the locale of the page
    pub block_name: Option<String>,
    pub teams: Vec<EventTeamRow>,
    pub games: Vec<GameRow>,
    pub streams: Vec<StreamRow>,
}

/// A slot of a match: its team, wins and outcome always come from the same answer
#[derive(Debug, Clone, PartialEq)]
pub struct EventTeamRow {
    /// 1 or 2, order of the answer (game 1's blue team first once sides are picked)
    pub slot: i32,
    /// `None` = TBD
    pub team_ext_id: Option<i64>,
    pub game_wins: Option<i32>,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameRow {
    pub ext_id: i64,
    pub number: i32,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamRow {
    pub provider: String,
    pub parameter: String,
    pub locale: String,
    pub english_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeagueStub {
    pub ext_id: i64,
    pub slug: String,
    pub name: String,
    pub image_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamStub {
    pub ext_id: i64,
    pub name: String,
    pub code: String,
    pub image_url: String,
}

/// Values the API sends that the CHECKs in `schema.sql` accept (a test keeps them equal).
/// An event with any other value is skipped by the adapter, so it can't fail a whole page
pub const EVENT_STATES: &[&str] = &["unstarted", "inProgress", "completed"];
pub const GAME_STATES: &[&str] = &["unstarted", "inProgress", "completed", "unneeded"];
pub const STRATEGIES: &[&str] = &["bestOf", "playAll"];
pub const MATCH_TYPES: &[&str] = &["normal", "tiebreaker"];
/// `tie`: a `playAll 2` ending 1-1
pub const OUTCOMES: &[&str] = &["win", "loss", "tie"];

/// How far an event got
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Progress {
    pub state_rank: i32,
    pub wins: i64,
    pub outcomes: i64,
}

impl Progress {
    /// Any part lower than what we have: an old copy, skipped (ADR 0005). State, wins and
    /// outcomes never go down in a real answer
    pub fn went_back_from(&self, stored: &Progress) -> bool {
        self.state_rank < stored.state_rank
            || self.wins < stored.wins
            || self.outcomes < stored.outcomes
    }
}

/// `unstarted` 0, `inProgress` 1, `completed` (and a game's `unneeded`) 2. Same as
/// `v2_state_rank()` in `schema.sql`
pub fn state_rank(state: &str) -> i32 {
    match state {
        "inProgress" => 1,
        "completed" | "unneeded" => 2,
        _ => 0,
    }
}

impl EventRow {
    pub fn progress(&self) -> Progress {
        Progress {
            state_rank: state_rank(&self.state),
            wins: self
                .teams
                .iter()
                .map(|t| i64::from(t.game_wins.unwrap_or(0)))
                .sum(),
            outcomes: self.teams.iter().filter(|t| t.outcome.is_some()).count() as i64,
        }
    }
}
