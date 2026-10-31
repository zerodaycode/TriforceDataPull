use super::season::V2Season;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_season_i18n")]
pub struct V2SeasonI18n {
    #[primary_key]
    pub id: i32,
    #[foreign_key(references = V2Season::id)]
    pub season_id: i32,
    pub locale: String,
    pub name: String,
}
