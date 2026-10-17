use canyon_sql::macros::*;
use serde::Serialize;

use crate::client::legacy::models as api;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
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

impl From<api::League> for League {
    fn from(value: api::League) -> Self {
        Self {
            id: Default::default(),
            ext_id: value.id.into(),
            slug: value.slug,
            name: value.name,
            region: value.region,
            image_url: value.image,
        }
    }
}

impl From<&api::League> for League {
    fn from(value: &api::League) -> Self {
        Self {
            id: Default::default(),
            ext_id: value.id.into(),
            slug: value.slug.clone(),
            name: value.name.clone(),
            region: value.region.clone(),
            image_url: value.image.clone(),
        }
    }
}
