use super::game::V2Game;
use super::player::V2Player;
use super::team::V2Team;
use canyon_sql::macros::*;
use serde::Serialize;

/// A player in a game: champion, role of that game and the last frame of the feed
#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_game_player")]
pub struct V2GamePlayer {
    #[primary_key]
    pub id: i32,
    #[foreign_key(table = "v2_game", column = "id")]
    pub game_id: i32,
    #[foreign_key(table = "v2_team", column = "id")]
    pub team_id: i32,
    // NULL when the feed has no esports id for the player (missing or "-")
    #[foreign_key(table = "v2_player", column = "id")]
    pub player_id: Option<i32>,
    // as the feed sends it, with the team code
    pub summoner_name: Option<String>,
    pub participant_id: i32,
    pub role: String,
    pub champion: String,
    pub level: Option<i32>,
    pub kills: Option<i32>,
    pub deaths: Option<i32>,
    pub assists: Option<i32>,
    pub gold: Option<i64>,
    pub cs: Option<i32>,
    pub wards_placed: Option<i32>,
    pub wards_destroyed: Option<i32>,
    // Canyon migrations have no floats: share × 10000
    pub damage_share_bp: Option<i32>,
    pub items: Option<String>,
    pub rune_style: Option<i32>,
    pub rune_sub_style: Option<i32>,
    pub runes: Option<String>,
    pub skill_order: Option<String>,
}
