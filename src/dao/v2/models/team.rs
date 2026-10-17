use super::league::V2League;
use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_team")]
pub struct V2Team {
    #[primary_key]
    pub id: i32,
    pub ext_id: i64,
    pub slug: String,
    pub name: String,
    pub code: String,
    pub image_url: String,
    // no source in the new API yet, NULL for now (ADR 0004)
    pub light_image_url: Option<String>,
    pub bg_image_url: Option<String>,
    // no source in the new API yet, NULL for now (ADR 0004)
    pub alt_image_url: Option<String>,
    #[foreign_key(table = "v2_league", column = "id")]
    pub home_league_id: Option<i32>,
}
