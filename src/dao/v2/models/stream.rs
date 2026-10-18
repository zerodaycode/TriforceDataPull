use super::event::V2Event;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_stream")]
pub struct V2Stream {
    #[primary_key]
    pub id: i32,
    #[foreign_key(table = "v2_event", column = "id")]
    pub event_id: i32,
    pub provider: String,
    pub parameter: String,
    pub locale: String,
    pub english_name: String,
}
