use super::player::V2Player;
use super::team::V2Team;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_team_player")]
pub struct V2TeamPlayer {
    #[primary_key]
    pub id: i32,
    #[foreign_key(references = V2Team::id)]
    pub team_id: i32,
    #[foreign_key(references = V2Player::id)]
    pub player_id: i32,
}
