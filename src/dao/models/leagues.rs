use serde::Serialize;
use canyon_sql::macros::*;

use crate::data_pull;

#[derive(Debug, Clone, CanyonCrud, CanyonMapper, Serialize)]
#[canyon_entity]
pub struct League {
    #[primary_key]
    id: i32,
    ext_id: i64,
    slug: String,
    name: String,
    region: String,
    image_url: String,
}

impl From<data_pull::serde_models::League> for League {
    fn from(value: data_pull::serde_models::League) -> Self {
        Self {
            id: Default::default(),
            ext_id: value.id.into(),
            slug: value.slug,
            name: value.name,
            region: value.region,
            image_url: value.image
        }
    }
}
