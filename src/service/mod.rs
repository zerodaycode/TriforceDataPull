pub mod caller;
use crate::{utils::constants::lolesports, data_pull::serde_models::{Wrapper, Leagues, LeagueForTournaments, Tournaments}};

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
    leagues: Leagues,
    tournaments: OurTournaments,
    // teams: Vec<serde_models::Team>,
    // schedule: Vec<serde_models::Event>,
}

/// TODO Docs
// #[derive(Default, Debug)]
// pub struct NewTournaments(Vec<Tournaments>);


impl DataPull {
    
    pub async fn new() -> Self {
        Self {
            leagues: Leagues::default(),
            tournaments: OurTournaments::default(),
        }
    }

    pub async fn fetch_leagues(&mut self) {
        let response = caller::make_get_request(lolesports::LEAGUES_ENDPOINT, &[]).await.expect("Cant unwrap the result");

        match serde_json::from_str::<Wrapper<Leagues>>(&response.text().await.unwrap()) {
            Ok(parsed) => self.leagues = parsed.data,
            Err(e) => println!("{:?}",e),
        };
    }

    pub async fn fetch_tournaments(&mut self) {
        for league in &self.leagues.leagues {

            let response = caller::make_get_request(lolesports::TOURNAMENTS_ENDPOINT, &[("leagueId", league.id.clone())]).await.expect("Couldn't unwrap the result");

            match serde_json::from_str::<Wrapper<LeagueForTournaments>>(&response.text().await.unwrap()) {
                Ok(parsed) => {
                    let mut tournaments_in_league = parsed.data.leagues[0].clone();
                    tournaments_in_league.league_id = league.id.clone();
                    self.tournaments.push(tournaments_in_league)
                },
                Err(e) => println!("{:?}", e),
            };
        }

    }
    
}