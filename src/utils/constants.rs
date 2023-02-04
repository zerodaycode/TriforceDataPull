//! Constant values definitions for `Triforce`

pub mod lolesports {
    pub static base_url: &str =
        "https://esports-api.lolesports.com/persisted/gw";

    pub static LEAGUES_ENDPOINT: &str = "/getLeagues?hl=en-US";
    pub static TOURNAMENTS_ENDPOINT: &str = "/getTournamentsForLeague?hl=en-US";
    pub static TEAMS_AND_LEAGUES_ENDPOINT: &str = "/getTeams?hl=en-US";
}