use super::league::V2League;
use super::season::V2Season;
use super::split::V2Split;
use canyon_sql::{date_time::NaiveDateTime, macros::*};
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_tournament")]
pub struct V2Tournament {
    #[primary_key]
    pub id: i32,
    pub ext_id: i64,
    #[foreign_key(references = V2League::id)]
    pub league_id: i32,
    #[foreign_key(references = V2Season::id)]
    pub season_id: Option<i32>,
    #[foreign_key(references = V2Split::id)]
    pub split_id: Option<i32>,
    pub slug: String,
    pub start_time: Option<NaiveDateTime>,
    pub end_time: Option<NaiveDateTime>,
}
