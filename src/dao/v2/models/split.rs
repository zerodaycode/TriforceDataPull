use canyon_sql::macros::*;
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_split")]
pub struct V2Split {
    #[primary_key]
    pub id: i32,
    pub ext_id: i64,
}
