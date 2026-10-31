use canyon_sql::{date_time::NaiveDateTime, macros::*};
use serde::Serialize;

use super::leagues::League;
use super::teams::Team;
use crate::client::legacy::models as api;

#[derive(Debug, Clone, Crud, Fields, CanyonMapper, Serialize)]
#[canyon_entity]
pub struct Schedule {
    #[primary_key]
    id: i32,
    start_time: Option<NaiveDateTime>,
    state: String,
    event_type: String,
    blockname: Option<String>,
    // bigint foreign keys to int4 ids: the find_* methods Canyon generates for them fail, don't use them (ADR 0006)
    #[foreign_key(references = League::id)]
    league_id: Option<i64>,
    match_id: Option<i64>,
    strategy: Option<String>,
    strategy_count: Option<i64>,
    #[foreign_key(references = Team::id)]
    team_left_id: Option<i64>,
    team_left_wins: Option<i64>,
    team_right_id: Option<i64>,
    team_right_wins: Option<i64>,
}

impl From<&api::Event> for Schedule {
    fn from(value: &api::Event) -> Self {
        Self {
            id: Default::default(),
            start_time: Some(value.start_time.into()),
            state: value.state.clone(),
            event_type: value.r#type.clone(),
            blockname: value.block_name.clone(),
            league_id: None,
            match_id: value.r#match.as_ref().map(|r#match| r#match.id.into()),
            strategy: value
                .r#match
                .as_ref()
                .map(|r#match| r#match.strategy.r#type.clone()),
            strategy_count: value
                .r#match
                .as_ref()
                .map(|r#match| r#match.strategy.count.into()),
            team_left_id: None,
            team_left_wins: None,
            team_right_id: None,
            team_right_wins: None,
        }
    }
}

impl From<&api::EventDetails> for Schedule {
    fn from(value: &api::EventDetails) -> Self {
        Self {
            id: Default::default(),
            start_time: value.start_time.map(|ldt| ldt.into()),
            state: value.state.clone().unwrap_or_default(),
            event_type: value.r#type.clone(),
            blockname: value.blockname.clone(),
            league_id: None,
            match_id: match &value.r#type.as_str() {
                &"show" => None,
                _ => Some(value.id.into()),
            },
            strategy: value
                .r#match
                .as_ref()
                .map(|r#match| r#match.strategy.r#type.clone()),
            strategy_count: value
                .r#match
                .as_ref()
                .map(|r#match| r#match.strategy.count.into()),
            team_left_id: None,
            team_left_wins: None,
            team_right_id: None,
            team_right_wins: None,
        }
    }
}

impl Schedule {
    pub fn merge_with_event_details(&mut self, event_details: &api::EventDetails) {
        if let Some(first_team) = event_details
            .r#match
            .as_ref()
            .and_then(|event_match| event_match.teams.first())
            .and_then(|team| team.result.as_ref())
        {
            self.team_left_wins = Some(first_team.game_wins.into());
        }

        if let Some(second_team) = event_details
            .r#match
            .as_ref()
            .and_then(|event_match| event_match.teams.get(1))
            .and_then(|team| team.result.as_ref())
        {
            self.team_right_wins = Some(second_team.game_wins.into());
        }

        if let Some(new_state) = &event_details.state {
            self.state = new_state.clone();
        }
    }
}
