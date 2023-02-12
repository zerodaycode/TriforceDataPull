use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, CanyonCrud, CanyonMapper, Serialize, Default)]
#[canyon_entity]
pub struct TeamPlayer {
    #[primary_key]
    id: i32,
    team_id: Option<i64>,
    player_id: Option<i64>
}