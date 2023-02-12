use std::default;

use canyon_sql::macros::*;
use serde::Serialize;

use crate::data_pull;

#[derive(Debug, Clone, CanyonCrud, CanyonMapper, Serialize)]
#[canyon_entity]
pub struct Team {
    #[primary_key]
    id: i32,
    ext_id: i64,
    name: String,
    slug: String,
    code: String,
    image_url: String,
    alt_image_url: Option<String>,
    bg_image_url: Option<String>,
    home_league: Option<i64>
}

impl From<&data_pull::serde_models::Team> for Team {
    fn from(value: &data_pull::serde_models::Team) -> Self {
        Self { 
            id: Default::default(), 
            ext_id: value.id.into(),
            name: value.name.clone(),
             slug: value.slug.clone(),
              code: value.code.clone(),
               image_url: value.image.clone(),
                alt_image_url: value.alternative_image.clone(),
                 bg_image_url: value.background_image.clone(),
                  home_league: None
                }
    }
}