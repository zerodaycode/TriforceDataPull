use super::event::V2Event;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_game")]
pub struct V2Game {
    #[primary_key]
    pub id: i32,
    pub ext_id: i64,
    #[foreign_key(table = "v2_event", column = "id")]
    pub event_id: i32,
    pub number: i32,
    pub state: String,
}
