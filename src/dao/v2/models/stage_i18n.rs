use super::stage::V2Stage;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_stage_i18n")]
pub struct V2StageI18n {
    #[primary_key]
    pub id: i32,
    #[foreign_key(table = "v2_stage", column = "id")]
    pub stage_id: i32,
    pub locale: String,
    pub name: String,
}
