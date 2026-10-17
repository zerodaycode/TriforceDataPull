//! The site queries Triforce calls

pub mod lol_team_overview;
pub mod other_leagues;

use super::Operation;

/// A change in the query text of these is logged on refresh
pub const USED: &[&str] = &[
    other_leagues::GetOtherLeagues::NAME,
    lol_team_overview::GetLoLTeamOverview::NAME,
];
