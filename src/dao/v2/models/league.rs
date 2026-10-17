use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_league")]
pub struct V2League {
    #[primary_key]
    pub id: i32,
    pub ext_id: i64,
    pub slug: String,
    pub image_url: String,
    pub dark_image_url: Option<String>,
}
