use super::game::V2Game;
use super::team::V2Team;
use canyon_sql::macros::*;
use serde::Serialize;

/// A team in a game: its side and its totals in the last frame of the feed
#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_game_team")]
pub struct V2GameTeam {
    #[primary_key]
    pub id: i32,
    #[foreign_key(references = V2Game::id)]
    pub game_id: i32,
    #[foreign_key(references = V2Team::id)]
    pub team_id: i32,
    pub side: String,
    // `feed` (from the game itself) or `api` (watchVodQuery, no feed)
    pub side_source: String,
    // only known for sweeps (or live, later)
    pub result: Option<String>,
    pub gold: Option<i64>,
    pub kills: Option<i32>,
    pub towers: Option<i32>,
    pub inhibitors: Option<i32>,
    pub barons: Option<i32>,
    pub dragons: Option<String>,
}
