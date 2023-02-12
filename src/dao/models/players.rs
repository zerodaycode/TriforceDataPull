use canyon_sql::macros::*;
use serde::Serialize;

use crate::data_pull;

#[derive(Debug, Clone, CanyonCrud, CanyonMapper, Serialize)]
#[canyon_entity]
pub struct Player {
    #[primary_key]
    id: i32,
    ext_id: i64,
    first_name: String,
    last_name: String,
    summoner_name: String,
    image_url: Option<String>,
    role: String,
}

impl From<&data_pull::serde_models::Player> for Player {
    fn from(value: &data_pull::serde_models::Player) -> Self {
        Self { 
            id: Default::default(),
            ext_id: value.id.into(),
            first_name: value.first_name.clone(),
            last_name: value.last_name.clone(),
            summoner_name: value.summoner_name.clone(),
            image_url: value.image.clone(),
            role: value.role.clone()
        }
    }
}