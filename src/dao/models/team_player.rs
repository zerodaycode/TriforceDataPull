use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, CanyonCrud, CanyonMapper, Serialize, Default)]
#[canyon_entity(table_name = "team_player")]
pub struct TeamPlayer {
    #[primary_key]
    id: i32,
    team_id: Option<i64>,
    player_id: Option<i64>,
}
