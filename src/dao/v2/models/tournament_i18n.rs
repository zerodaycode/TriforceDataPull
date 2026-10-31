use super::tournament::V2Tournament;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_tournament_i18n")]
pub struct V2TournamentI18n {
    #[primary_key]
    pub id: i32,
    #[foreign_key(references = V2Tournament::id)]
    pub tournament_id: i32,
    pub locale: String,
    pub name: String,
}
