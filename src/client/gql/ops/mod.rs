//! The site queries Triforce calls

pub mod other_leagues;

use super::Operation;

/// A change in the query text of these is logged on refresh
pub const USED: &[&str] = &[other_leagues::GetOtherLeagues::NAME];
