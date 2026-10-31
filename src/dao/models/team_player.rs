use super::players::Player;
use super::teams::Team;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize, Default, Fields)]
#[canyon_entity(table_name = "team_player")]
pub struct TeamPlayer {
    #[primary_key]
    id: i32,
    // bigint foreign keys to int4 ids: the find_* methods Canyon generates for them fail, don't use them (ADR 0006)
    #[foreign_key(references = Team::id)]
    team_id: Option<i64>,
    #[foreign_key(references = Player::id)]
    player_id: Option<i64>,
}
