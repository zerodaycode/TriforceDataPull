use canyon_sql::macros::*;
use serde::Serialize;

use crate::client::legacy::models as api;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
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

impl From<&api::Player> for Player {
    fn from(value: &api::Player) -> Self {
        Self {
            id: Default::default(),
            ext_id: value.id.into(),
            first_name: value.first_name.clone(),
            last_name: value.last_name.clone(),
            summoner_name: value.summoner_name.clone(),
            image_url: value.image.clone(),
            role: value.role.clone(),
        }
    }
}
