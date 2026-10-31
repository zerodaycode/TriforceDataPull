//! The site queries Triforce calls

pub mod lol_team_overview;
pub mod other_leagues;
pub mod schedule_events;
pub mod season_tournament;
pub mod watch_live;
pub mod watch_vod;

use serde::{Deserialize, Deserializer, de::DeserializeOwned};

use super::Operation;

/// An event list item that keeps the ones that didn't decode
pub trait BadEvent: DeserializeOwned {
    fn bad(why: String) -> Self;
}

/// Each event on its own: one bad event can't lose the other 99 of the page
pub fn each_event<'de, D: Deserializer<'de>, E: BadEvent>(d: D) -> Result<Vec<E>, D::Error> {
    let raw = Option::<Vec<serde_json::Value>>::deserialize(d)?.unwrap_or_default();
    Ok(raw
        .into_iter()
        .map(|v| {
            let id = v
                .get("id")
                .and_then(|x| x.as_str())
                .unwrap_or("?")
                .to_string();
            serde_json::from_value(v).unwrap_or_else(|e| E::bad(format!("{id}: {e}")))
        })
        .collect())
}

/// A change in the query text of these is logged on refresh
pub const USED: &[&str] = &[
    other_leagues::GetOtherLeagues::NAME,
    lol_team_overview::GetLoLTeamOverview::NAME,
    season_tournament::GetSeasonTournament::NAME,
    schedule_events::GetScheduleEvents::NAME,
    watch_vod::GetWatchVod::NAME,
    watch_live::GetWatchLive::NAME,
];
