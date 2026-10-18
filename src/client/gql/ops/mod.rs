//! The site queries Triforce calls

pub mod lol_team_overview;
pub mod other_leagues;
pub mod schedule_events;
pub mod season_tournament;
pub mod watch_vod;

use super::Operation;

/// A change in the query text of these is logged on refresh
pub const USED: &[&str] = &[
    other_leagues::GetOtherLeagues::NAME,
    lol_team_overview::GetLoLTeamOverview::NAME,
    season_tournament::GetSeasonTournament::NAME,
    schedule_events::GetScheduleEvents::NAME,
    watch_vod::GetWatchVod::NAME,
];
