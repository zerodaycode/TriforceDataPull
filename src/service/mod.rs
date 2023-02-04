pub mod caller;
use crate::{utils::constants::lolesports, data_pull::serde_models::{Wrapper, Leagues, LeagueForTournaments, Tournaments, TeamsPlayers, Team, Player, LolesportsId, Schedule, ScheduleOutter}};
use color_eyre::{eyre::Context, Result};

/**
 * This type alias are just a communist joke. They are the Lolesports tournaments only?
 * Nope. They are OUR tournaments.
 */
type OurTournaments = Vec<Tournaments>;


/// Contains the operations against the `LolEsports` API to
/// fetch the content via REST request that `Triforce` needs
/// to pull, parse, handle and store.
#[derive(Debug)]
pub struct DataPull {
    pub leagues: Leagues,
    pub tournaments: OurTournaments,
    pub teams: Vec<Team>,
    pub players: Vec<Player>,
    pub schedule: ScheduleOutter
}

/// TODO Docs
// #[derive(Default, Debug)]
// pub struct NewTournaments(Vec<Tournaments>);


impl DataPull {
    
    pub async fn new() -> Self {
        Self {
            leagues: Leagues::default(),
            tournaments: OurTournaments::default(),
            teams: Vec::default(),
            players: Vec::default(),
            schedule: ScheduleOutter::default(),
        }
    }

    pub async fn fetch_leagues(&mut self) -> Result<()> {
        let response = caller::make_get_request::<&[()]>(lolesports::LEAGUES_ENDPOINT, None)
            .await
            .with_context(|| "A failure happened retrieving the Leagues from Lolesports");

        serde_json::from_str::<Wrapper<Leagues>>(&response?.text().await.unwrap())
            .map(|parsed| self.leagues = parsed.data)
            .with_context(|| "A failure happened parsing the Leagues from Lolesports")
    }

    pub async fn fetch_tournaments(&mut self) -> Result<()> {
        for league in &self.leagues.leagues {

            let response = caller::make_get_request(lolesports::TOURNAMENTS_ENDPOINT, Some(&[("leagueId", &league.id)]))
            .await
            .with_context(|| "A failure happened retrieving the Tournaments from Lolesports");

            serde_json::from_str::<Wrapper<LeagueForTournaments>>(&response?.text().await.unwrap())
                .map(|parsed| {
                    let mut tournaments_in_league = parsed.data.leagues[0].clone();
                    tournaments_in_league.league_id = league.id.clone();
                    self.tournaments.push(tournaments_in_league)
                })
                .with_context(|| "A failure happened parsing the Tournaments from Lolesports")?;
        }

        Ok(())
    }

    pub async fn fetch_teams_and_players(&mut self) -> Result<()> {
        let response = caller::make_get_request::<&[()]>(
            lolesports::TEAMS_AND_LEAGUES_ENDPOINT,
                None
            ).await
            .with_context(|| "A failure happened retrieving the Teams and players from Lolesports");

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

    pub async fn fetch_schedule(&mut self) -> Result<()> {
        let response = caller::make_get_request::<&[()]>(
            lolesports::SCHEDULE_ENDPOINT,
                None
            ).await
            .with_context(|| "A failure happened retrieving the schedule from Lolesports");

        serde_json::from_str::<Wrapper<ScheduleOutter>>(&response?.text().await.unwrap())
            .map(|parsed| self.schedule = parsed.data)
            .with_context(|| "A failure happened parsing the Schedule from Lolesports")
        }

    fn search_league_by_name(&self, name: &str) -> LolesportsId {
        self.leagues.leagues.iter()
            .find(|league| (*league.name).eq(name))
            .map(|league| league.id)
            .unwrap_or_default() 
    } 
}