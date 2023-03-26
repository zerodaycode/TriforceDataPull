//! Constant values definitions for `Triforce`

pub mod lolesports {
    pub static BASE_URL: &str = "https://esports-api.lolesports.com/persisted/gw";

    pub static LEAGUES_ENDPOINT: &str = "/getLeagues?hl=en-US";
    pub static TOURNAMENTS_ENDPOINT: &str = "/getTournamentsForLeague?hl=en-US";
    pub static TEAMS_AND_LEAGUES_ENDPOINT: &str = "/getTeams?hl=en-US";
    pub static SCHEDULE_ENDPOINT: &str = "/getSchedule?hl=en-US";
    pub static LIVE_ENDPOINT: &str = "/getLive?hl=en-US";
    pub static EVENT_DETAILS_ENDPOINT: &str = "/getEventDetails?hl=en-US";
}
