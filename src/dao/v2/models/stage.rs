use super::tournament::V2Tournament;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_stage")]
pub struct V2Stage {
    #[primary_key]
    pub id: i32,
    pub ext_id: i64,
    #[foreign_key(table = "v2_tournament", column = "id")]
    pub tournament_id: i32,
}
