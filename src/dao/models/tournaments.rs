use canyon_sql::{date_time::NaiveDate, macros::*};
use serde::Serialize;

use crate::client::legacy::models as api;

use super::leagues::League;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity]
pub struct Tournament {
    #[primary_key]
    id: i32,
    ext_id: i64,
    slug: String,
    start_date: NaiveDate,
    end_date: NaiveDate,
    #[foreign_key(references = League::id)]
    league: i32,
}

impl From<api::Tournament> for Tournament {
    fn from(value: api::Tournament) -> Self {
        Self {
            id: Default::default(),
            ext_id: value.id.into(),
            slug: value.slug,
            start_date: value.start_date,
            end_date: value.end_date,
            league: Default::default(),
        }
    }
}

impl From<&api::Tournament> for Tournament {
    fn from(value: &api::Tournament) -> Self {
        Self {
            id: Default::default(),
            ext_id: value.id.into(),
            slug: value.slug.clone(),
            start_date: value.start_date,
            end_date: value.end_date,
            league: Default::default(),
        }
    }
}
