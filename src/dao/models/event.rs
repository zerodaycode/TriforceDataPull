
use serde::Serialize;
use canyon_sql::{macros::*,date_time::NaiveDateTime};


use crate::data_pull;

#[derive(Debug, Clone, CanyonCrud, CanyonMapper, Serialize)]
#[canyon_entity]
pub struct Schedule {
    #[primary_key]
    id: i32,
    start_time: Option<NaiveDateTime>,
    state: String,
    event_type: String,
    blockname: Option<String>,
    league_id: Option<i64>,
    match_id: Option<i64>,
    strategy: Option<String>,
    strategy_count: Option<i64>,
    team_left_id: Option<i64>,
    team_left_wins: Option<i64>,
    team_right_id: Option<i64>,
    team_right_wins: Option<i64>
}

impl From<&data_pull::serde_models::Event> for Schedule {
    fn from(value: &data_pull::serde_models::Event) -> Self {
        Self { 
            id: Default::default(),
            start_time: Some(value.start_time.into()),
            state: value.state.clone(),
            event_type: value.r#type.clone(),
            blockname: value.block_name.clone(),
            league_id: None,
            match_id: match &value.r#match {
                Some(r#match) => Some(r#match.id.into()),
                None => None,
            },
            strategy: match &value.r#match {
                Some(r#match) => Some(r#match.strategy.r#type.clone()),
                None => None,
            },
            strategy_count: match &value.r#match {
                Some(r#match) => Some(r#match.strategy.count.into()),
                None => None,
            },
            team_left_id: None,
            team_left_wins: None,
            team_right_id: None,
            team_right_wins: None
        }
    }
}
