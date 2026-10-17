use canyon_sql::macros::*;
use serde::Serialize;

use super::event::Schedule;
use crate::client::legacy::models as api;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize, Fields)]
#[canyon_entity]
pub struct Stream {
    #[primary_key]
    id: i32,
    #[foreign_key(table = "schedule", column = "id")]
    event_id: i32,
    provider: String,
    parameter: String,
    locale: String,
    english_name: String,
}

impl From<&api::Stream> for Stream {
    fn from(value: &api::Stream) -> Self {
        Self {
            id: Default::default(),
            event_id: Default::default(),
            provider: value.provider.clone(),
            parameter: value.parameter.clone(),
            locale: value.media_locale.locale.clone(),
            english_name: value.media_locale.english_name.clone(),
        }
    }
}
