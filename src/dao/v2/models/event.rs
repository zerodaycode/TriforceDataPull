use super::league::V2League;
use super::tournament::V2Tournament;
use canyon_sql::{date_time::NaiveDateTime, macros::*};
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_event")]
pub struct V2Event {
    #[primary_key]
    pub id: i32,
    pub ext_id: i64,
    pub kind: String,
    #[foreign_key(references = V2League::id)]
    pub league_id: i32,
    #[foreign_key(references = V2Tournament::id)]
    pub tournament_id: Option<i32>,
    pub start_time: Option<NaiveDateTime>,
    pub state: String,
    pub strategy_type: Option<String>,
    pub strategy_count: Option<i32>,
    pub match_type: Option<String>,
}
