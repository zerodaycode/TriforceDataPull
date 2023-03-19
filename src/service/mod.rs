pub mod caller;

use chrono::Local;
use crate::{
    data_pull::serde_models::{
        Event, LeagueForTournaments, Leagues, LolesportsId, Player, ScheduleOutter, Team,
        TeamsPlayers, Tournament, Wrapper,
    },
    utils::constants::lolesports,
};
use color_eyre::{eyre::Context, Result};

/**
 * This type alias are just a communist joke. They are the Lolesports tournaments only?
 * Nope. They are OUR tournaments.
 */
pub type OurTournaments = Vec<Tournament>;

/// Contains the operations against the `LolEsports` API to
/// fetch the content via REST request that `Triforce` needs
/// to pull, parse, handle and store.
#[derive(Debug, Default)]
pub struct DataPull {
    pub leagues: Leagues,
    pub tournaments: OurTournaments,
    pub teams: Vec<Team>,
    pub players: Vec<Player>,
    pub schedule: Vec<Event>,
    pub live: Vec<Event>,
}

impl DataPull {
    pub async fn fetch_leagues(&mut self) -> Result<()> {

        println!("{} - Fetching Leagues from The LoLEsports API", Local::now().format("%Y-%m-%d %H:%M:%S.%f"));
        let response = caller::make_get_request::<&[()]>(lolesports::LEAGUES_ENDPOINT, None)
            .await
            .with_context(|| "A failure happened retrieving the Leagues from Lolesports");

        serde_json::from_str::<Wrapper<Leagues>>(&response?.text().await.unwrap())
            .map(|parsed| self.leagues = parsed.data)
            .with_context(|| "A failure happened parsing the Leagues from Lolesports")
    }

    pub async fn fetch_tournaments(&mut self) -> Result<()> {

        println!("{} - Tournaments Leagues from The LoLEsports API", Local::now().format("%Y-%m-%d %H:%M:%S.%f"));
        for league in &self.leagues.leagues {
            let response = caller::make_get_request(
                lolesports::TOURNAMENTS_ENDPOINT,
                Some(&[("leagueId", &league.id)]),
            )
            .await
            .with_context(|| "A failure happened retrieving the Tournaments from Lolesports");

            serde_json::from_str::<Wrapper<LeagueForTournaments>>(&response?.text().await.unwrap())
                .map(|parsed| {
                    let mut tournaments_in_league = parsed.data.leagues[0].clone();
                    tournaments_in_league
                        .tournaments
                        .iter_mut()
                        .for_each(|e| e.league_id = league.id);
                    self.tournaments.extend(tournaments_in_league.tournaments)
                })
                .with_context(|| "A failure happened parsing the Tournaments from Lolesports")?;
        }

        Ok(())
    }

    pub async fn fetch_teams_and_players(&mut self) -> Result<()> {

        println!("{} - Fetching Teams and Players from The LoLEsports API", Local::now().format("%Y-%m-%d %H:%M:%S.%f"));
        let response =
            caller::make_get_request::<&[()]>(lolesports::TEAMS_AND_LEAGUES_ENDPOINT, None)
                .await
                .with_context(|| {
                    "A failure happened retrieving the Teams and players from Lolesports"
                });

        serde_json::from_str::<Wrapper<TeamsPlayers>>(&response?.text().await.unwrap())
            .map(|parsed| {
                for mut team in parsed.data.teams {
                    if let Some(home_league) = &mut team.home_league {
                        home_league.league_id = self.search_league_by_name(&home_league.name);
                    }
                    self.teams.push(team.clone());
                    self.players.extend(team.players.into_iter())
                }
            })
            .with_context(|| "A failure happened parsing the Tournaments from Lolesports")
    }

    // pub async fn fetch_schedule(&mut self) -> Result<()> {
    //     let response = caller::make_get_request::<&[()]>(
    //         lolesports::SCHEDULE_ENDPOINT,
    //             None
    //         ).await
    //         .with_context(|| "A failure happened retrieving the schedule from Lolesports");

    //     serde_json::from_str::<Wrapper<ScheduleOutter>>(&response?.text().await.unwrap())
    //         .map(|parsed| self.schedule = parsed.data)
    //         .with_context(|| "A failure happened parsing the Schedule from Lolesports")
    // }

    // FIX ME Right now It only fetch present and future matches, not past
    pub async fn process_full_schedule(&mut self) -> Result<()> {

        println!("{} - Fetching Full schedule from The LoLEsports API", Local::now().format("%Y-%m-%d %H:%M:%S.%f"));
        self.fetch_full_schedule().await?;
        // process
        for event in self.schedule.iter_mut() {
            event.league.league_id = self
                .leagues
                .leagues
                .iter()
                .find(|league| (*league.slug).eq(&event.league.slug))
                .map(|league| league.id)
                .unwrap_or_default();
        }
        Ok(())
    }

    async fn fetch_full_schedule(&mut self) -> Result<()> {
        let first_response = caller::make_get_request::<&[()]>(lolesports::SCHEDULE_ENDPOINT, None)
            .await
            .with_context(|| "A failure happened retrieving the schedule from Lolesports");

        let schedule_first_page =
            serde_json::from_str::<Wrapper<ScheduleOutter>>(&first_response?.text().await.unwrap())
                .with_context(|| "Error retrieving the first page of the schedule")?;
        // Appending the already downloaded first paginated resource of the schedule
        self.schedule
            .extend(schedule_first_page.data.schedule.events);

        let mut newer_entry_sentinel = schedule_first_page.data.schedule.pages.newer;
        let mut total_new_entries = 1;
        // While the API returns a key with newer entries, we will continue fetching the calendar
        while let Some(newer_events) = &newer_entry_sentinel {
            let r = caller::make_get_request(
                lolesports::SCHEDULE_ENDPOINT,
                Some(&[("pageToken", newer_events)]),
            )
            .await
            .with_context(|| "A failure happened retrieving the schedule from Lolesports");

            serde_json::from_str::<Wrapper<ScheduleOutter>>(&r?.text().await.unwrap())
                .map(|parsed| {
                    total_new_entries += 1;
                    println!("Requesting pages: {:?}", &parsed.data.schedule.pages);
                    println!("Total new entries fetched: {:?}", &total_new_entries);
                    newer_entry_sentinel = parsed.data.schedule.pages.newer;
                    self.schedule.extend(parsed.data.schedule.events)
                })
                .with_context(|| "A failure happened parsing the Schedule from Lolesports")?;
        }

        Ok(())
    }

    fn search_league_by_name(&self, name: &str) -> LolesportsId {
        self.leagues
            .leagues
            .iter()
            .find(|league| (*league.name).eq(name))
            .map(|league| league.id)
            .unwrap_or_default()
    }

    pub async fn fetch_live(&mut self) -> Result<()> {
        let response = caller::make_get_request::<&[()]>(lolesports::LIVE_ENDPOINT, None)
            .await
            .with_context(|| "A failure happened retrieving the Live Events from Lolesports");

        serde_json::from_str::<Wrapper<ScheduleOutter>>(&response?.text().await.unwrap())
            .map(|parsed| self.live = parsed.data.schedule.events)
            .with_context(|| "A failure happened parsing the Live Events from Lolesports")
    }
}
