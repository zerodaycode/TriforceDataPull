use super::players::Player;
use super::teams::Team;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, CanyonCrud, CanyonMapper, Serialize, Default, Fields)]
#[canyon_entity(table_name = "team_player")]
pub struct TeamPlayer {
    #[primary_key]
    id: i32,
    #[foreign_key(table = "team", column = "id")]
    team_id: Option<i64>,
    #[foreign_key(table = "player", column = "id")]
    player_id: Option<i64>,
}
