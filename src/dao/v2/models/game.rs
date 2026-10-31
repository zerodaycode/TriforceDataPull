use super::event::V2Event;
use canyon_sql::{date_time::NaiveDateTime, macros::*};
use serde::Serialize;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_game")]
pub struct V2Game {
    #[primary_key]
    pub id: i32,
    pub ext_id: i64,
    #[foreign_key(references = V2Event::id)]
    pub event_id: i32,
    pub number: i32,
    pub state: String,
    // from the live feed when the game is done (game details)
    pub patch: Option<String>,
    pub start_time: Option<NaiveDateTime>,
    pub end_time: Option<NaiveDateTime>,
    // NULL = not fetched yet, `done`, `no_feed` (sides from watchVodQuery)
    pub details: Option<String>,
    pub details_tries: Option<i32>,
    // a failed game waits until then: 10 min, 40 min, 2 h 40, 10 h 40
    pub details_next_try: Option<NaiveDateTime>,
}
