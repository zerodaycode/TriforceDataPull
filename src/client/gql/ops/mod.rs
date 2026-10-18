//! The site queries Triforce calls

pub mod lol_team_overview;
pub mod other_leagues;
pub mod schedule_events;
pub mod season_tournament;

use super::Operation;

/// A change in the query text of these is logged on refresh
pub const USED: &[&str] = &[
    other_leagues::GetOtherLeagues::NAME,
    lol_team_overview::GetLoLTeamOverview::NAME,
    season_tournament::GetSeasonTournament::NAME,
    schedule_events::GetScheduleEvents::NAME,
];

/// GraphQL sends `null`, not a missing key: treat it as empty
pub(crate) fn null_as_empty<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    use serde::Deserialize;
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}
