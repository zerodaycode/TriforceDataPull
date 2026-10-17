use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_player")]
pub struct V2Player {
    #[primary_key]
    pub id: i32,
    pub ext_id: i64,
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub image_url: Option<String>,
    pub role: String,
}
