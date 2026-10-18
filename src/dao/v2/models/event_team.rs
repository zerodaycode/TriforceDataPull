use super::event::V2Event;
use super::team::V2Team;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_event_team")]
pub struct V2EventTeam {
    #[primary_key]
    pub id: i32,
    #[foreign_key(table = "v2_event", column = "id")]
    pub event_id: i32,
    pub slot: i32,
    // NULL while the slot is TBD
    #[foreign_key(table = "v2_team", column = "id")]
    pub team_id: Option<i32>,
    pub game_wins: Option<i32>,
    pub outcome: Option<String>,
}
