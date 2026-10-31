use super::league::V2League;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_league_i18n")]
pub struct V2LeagueI18n {
    #[primary_key]
    pub id: i32,
    #[foreign_key(references = V2League::id)]
    pub league_id: i32,
    pub locale: String,
    pub name: String,
    pub region: String,
    pub position: Option<i32>,
}
