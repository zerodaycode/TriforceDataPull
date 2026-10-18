use super::event::V2Event;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_event_i18n")]
pub struct V2EventI18n {
    #[primary_key]
    pub id: i32,
    #[foreign_key(table = "v2_event", column = "id")]
    pub event_id: i32,
    pub locale: String,
    pub block_name: String,
}
