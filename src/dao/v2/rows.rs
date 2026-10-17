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
