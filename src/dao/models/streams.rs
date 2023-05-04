use canyon_sql::macros::*;
use serde::Serialize;

use super::event::Schedule;
use crate::data_pull;

#[derive(Debug, Clone, CanyonCrud, CanyonMapper, Serialize, Fields)]
#[canyon_entity]
pub struct Stream {
    #[primary_key]
    id: i32,
    #[foreign_key(table = "schedule", column = "id")]
    event_id: i32,
    provider : String,
    parameter: String,
    locale: String,
    english_name: String,

}


impl From<&data_pull::serde_models::Stream> for Stream {
    fn from(value: &data_pull::serde_models::Stream) -> Self {
        Self {
            id: Default::default(),
            event_id: Default::default(),
            provider : value.provider.clone(),
            parameter: value.parameter.clone(),
            locale: value.media_locale.locale.clone(),
            english_name: value.media_locale.english_name.clone(),
        }
    }
}
